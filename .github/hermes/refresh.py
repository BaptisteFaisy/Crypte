#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""Refresh automatique des PR ouvertes ([REFRESH-AUTO], demande user 2026-11-07).

Déclenché quand `main` bouge (push) : c'est le chaînon manquant — ni Mergify
(qui ne met à jour qu'à l'entrée en file) ni hermes-lite (dry-run : il
évalue, il ne touche à rien) ne rafraîchit les branches au fil de l'eau.
Ce script ne merge JAMAIS : Mergify reste le seul pilote de merge.

- PR en retard sans conflit → rebase serveur (PUT update-branch).
- PR en conflit → label `hermes/conflit` + commentaire de diagnostic
  (fichiers en conflit via un merge d'essai local + commandes de fix).
- PR redevenue fraîche → label retiré, commentaire marqué résolu.
- Label `hermes/intervention-humaine` → PR ignorée (un humain s'en occupe).

Après un rebase serveur, les runs garés en `action_required` (push de bot,
[NO-APPROVAL-LOCK]) sont approuvés par le script lui-même.

Usage CI : `GH_TOKEN`, `GITHUB_REPOSITORY`, `GITHUB_EVENT_NAME`,
`GITHUB_EVENT_PATH` (ou `INPUT_PR_NUMBER` en dispatch manuel).
`--dry-run` : lectures seules, affiche les actions sans rien écrire.
"""

import json
import os
import shutil
import subprocess
import sys
import tempfile

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from gate import LABEL_CONFLIT, api, list_open_prs, refresh_branch, set_label

LABEL_HUMAIN = 'hermes/intervention-humaine'
MARKER = '<!-- crypte-refresh -->'
STATE_CONFLICT = '<!-- crypte-refresh-state: conflict -->'
STATE_RESOLVED = '<!-- crypte-refresh-state: resolved -->'
CLONE_TIMEOUT = 180
GIT_TIMEOUT = 60


def valid_repo(repo):
    """`owner/name` GitHub (caractères autorisés uniquement)."""
    import re
    return re.fullmatch(r'[A-Za-z0-9_.-]+/[A-Za-z0-9_.-]+', repo or '') is not None


def valid_sha(sha):
    """SHA hexadécimal (complet ou abrégé)."""
    import re
    return re.fullmatch(r'[0-9a-f]{4,64}', sha or '') is not None


def pr_details(repo, token, num):
    """PR fraîche (l'endpoint de liste ne calcule pas `mergeable`)."""
    sc, pr = api('GET', '/repos/%s/pulls/%d' % (repo, num), token=token)
    return pr if sc == 200 and isinstance(pr, dict) else None


def behind_by(repo, token, pr):
    """Commits de retard sur la base (-1 si incomparable)."""
    sc, cmp_ = api('GET', '/repos/%s/compare/%s...%s'
                   % (repo, pr['base']['ref'], pr['head']['sha']), token=token)
    behind = cmp_.get('behind_by') if sc == 200 else -1
    return behind if isinstance(behind, int) else -1


def git_auth_header(repo, token):
    """Valeur `http.extraHeader` pour cloner le dépôt (privé).

    Réutilise celle posée par `actions/checkout` (persist-credentials),
    sinon la reconstruit depuis le jeton. Ne jamais l'afficher.
    """
    try:
        r = subprocess.run(['git', 'config', '--get',
                            'http.https://github.com/.extraheader'],
                           capture_output=True, text=True, check=False,
                           timeout=GIT_TIMEOUT)
    except subprocess.TimeoutExpired:
        r = None
    if r is not None and r.returncode == 0 and r.stdout.strip():
        return r.stdout.strip()
    import base64
    raw = ('x-access-token:' + token).encode()
    return 'AUTHORIZATION: basic ' + base64.b64encode(raw).decode()


def conflicting_files(repo, token, pr):
    """Fichiers en conflit via un merge d'essai dans un clone jetable.

    Rend la liste des chemins, [] si le merge passe (GitHub avait un
    `mergeable` périmé), ou None si le diagnostic a échoué. Le clone
    part de GitHub (jamais du checkout local : l'état des runners
    self-hosted n'est pas fiable) et est détruit après usage.
    """
    num, base_ref = pr['number'], pr['base']['ref']
    sc, base = api('GET', '/repos/%s/git/ref/heads/%s' % (repo, base_ref),
                   token=token)
    if sc != 200 or not isinstance(base, dict):
        return None
    main_sha = ((base.get('object') or {}).get('sha') or '')
    if not valid_sha(main_sha):
        return None
    header = git_auth_header(repo, token)
    tmp = tempfile.mkdtemp(prefix='crypte-refresh-')
    clone = os.path.join(tmp, 't')
    try:
        url = 'https://github.com/%s.git' % repo
        cfg = ['-c', 'http.extraHeader=' + header]
        try:
            if subprocess.run(['git'] + cfg + ['clone', '-q', '--no-checkout',
                                               url, clone],
                              capture_output=True, check=False,
                              timeout=CLONE_TIMEOUT).returncode != 0:
                return None
        except subprocess.TimeoutExpired:
            return None

        def run(*a):
            try:
                return subprocess.run(['git'] + cfg + list(a), cwd=clone,
                                      capture_output=True, text=True,
                                      check=False, timeout=GIT_TIMEOUT)
            except subprocess.TimeoutExpired:
                return subprocess.CompletedProcess(a, 124)

        if run('fetch', '-q', 'origin', 'pull/%d/head' % num).returncode != 0:
            return None
        if run('checkout', '-q', '--detach', 'FETCH_HEAD').returncode != 0:
            return None
        run('merge', '--no-commit', '--no-ff', '-q', main_sha)
        r = run('diff', '--name-only', '-z', '--diff-filter=U')
        if r.returncode != 0:
            return None
        return sorted(p for p in r.stdout.split('\0') if p)
    finally:
        shutil.rmtree(tmp, ignore_errors=True)


def find_marker_comment(repo, token, num):
    """Commentaire de diagnostic existant (id, body), ou (None, '')."""
    sc, comments = api('GET', '/repos/%s/issues/%d/comments?per_page=100'
                       % (repo, num), token=token)
    if sc != 200 or not isinstance(comments, list):
        return None, ''
    for c in comments:
        if MARKER in (c.get('body') or ''):
            return c['id'], c['body']
    return None, ''


def upsert_comment(repo, token, num, body):
    """Crée ou met à jour l'unique commentaire de diagnostic."""
    cid, _ = find_marker_comment(repo, token, num)
    if cid:
        api('PATCH', '/repos/%s/issues/comments/%d' % (repo, cid),
            token=token, body={'body': body})
    else:
        api('POST', '/repos/%s/issues/%d/comments' % (repo, num),
            token=token, body={'body': body})


def conflict_body(pr, files):
    """Diagnostic de conflit : fichiers + commandes de fix."""
    listed = '\n'.join('- `%s`' % f for f in files) if files else \
        '- (fichiers non déterminés — rebase manuel requis)'
    return ('## Conflit avec `%s` — diagnostic automatique\n\n'
            'Fichiers en conflit :\n%s\n\n'
            '```sh\ngit fetch origin && git checkout %s && \\\n'
            '  git rebase origin/%s  # résoudre, puis --continue\n'
            'git push --force-with-lease\n```\n\n'
            'Ce commentaire est mis à jour automatiquement (`refresh-prs`) ;\n'
            'il passe à « résolu » quand la branche redevient fusionnable.\n%s\n%s'
            % (pr['base']['ref'], listed, pr['head']['ref'],
               pr['base']['ref'], MARKER, STATE_CONFLICT))


def resolved_body(pr):
    """Marque le diagnostic précédent comme résolu."""
    return ('Branche à jour sur `%s` ✓ — le conflit signalé précédemment '
            'est résolu (diagnostic automatique `refresh-prs`).\n%s\n%s'
            % (pr['base']['ref'], MARKER, STATE_RESOLVED))


def approve_parked_runs(repo, token, sha):
    """Approuve les runs garés par le push de bot ([NO-APPROVAL-LOCK]).

    Non fatal : si le jeton ne peut pas approuver, Mergify rejoue la CI
    sur ses branches de file de toute façon.
    """
    sc, data = api('GET', '/repos/%s/actions/runs?head_sha=%s&per_page=50'
                   % (repo, sha), token=token)
    if sc != 200 or not isinstance(data, dict):
        return
    for run in data.get('workflow_runs') or []:
        if run.get('status') != 'action_required':
            continue
        sc, _ = api('POST', '/repos/%s/actions/runs/%d/approve'
                    % (repo, run['id']), token=token)
        print('[refresh] run %d approuvé → %s' % (run['id'], sc), flush=True)


def mark_conflict(repo, token, pr, dry):
    """Label + diagnostic de conflit. Rend 'conflict'.

    Le merge d'essai arbitre : s'il passe, le `mergeable` de GitHub
    était périmé et on tente le rebase serveur au lieu d'étiqueter.
    """
    num = pr['number']
    files = conflicting_files(repo, token, pr)
    if files == []:
        # Essai propre : le `mergeable` était périmé, on tente le rebase
        # UNE fois, sans re-diagnostiquer (pas de récursion).
        print('[refresh] #%d : GitHub disait conflit, essai propre → rebase'
              % num, flush=True)
        if dry:
            return 'dry-refresh'
        r = refresh_branch(repo, token, pr)
        print('[refresh] #%d : rebase serveur → %s' % (num, r), flush=True)
        if r == 'refreshed':
            fresh = pr_details(repo, token, num)
            if fresh is not None:
                approve_parked_runs(repo, token, fresh['head']['sha'])
                return mark_fresh(repo, token, fresh, dry)
            return 'refreshed'
        files = None  # serveur têtu : diagnostic sans liste de fichiers
    print('[refresh] #%d : CONFLIT (%s)'
          % (num, ', '.join(files) if files else 'diagnostic impossible'),
          flush=True)
    if dry:
        return 'conflict'
    set_label(pr, repo, token, LABEL_CONFLIT, True)
    upsert_comment(repo, token, num, conflict_body(pr, files or []))
    return 'conflict'


def try_refresh(repo, token, pr, dry):
    """Rebase serveur + approbation des runs garés. Rend le verdict."""
    num = pr['number']
    r = refresh_branch(repo, token, pr)
    print('[refresh] #%d : rebase serveur → %s' % (num, r), flush=True)
    if r == 'refreshed':
        fresh = pr_details(repo, token, num)
        if fresh is not None:
            approve_parked_runs(repo, token, fresh['head']['sha'])
            return mark_fresh(repo, token, fresh, dry)
        return 'refreshed'
    if r == 'conflict':
        pr = pr_details(repo, token, num) or pr
        return mark_conflict(repo, token, pr, dry)
    return r


def mark_fresh(repo, token, pr, dry):
    """Retire le label ; bascule le diagnostic en « résolu »."""
    if dry:
        return 'fresh'
    set_label(pr, repo, token, LABEL_CONFLIT, False)
    _, body = find_marker_comment(repo, token, pr['number'])
    if body and STATE_RESOLVED not in body:
        upsert_comment(repo, token, pr['number'], resolved_body(pr))
    return 'fresh'


def handle_pr(repo, token, num, dry=False):
    """Rafraîchit une PR : 'refreshed' / 'conflict' / 'fresh' / autre."""
    pr = pr_details(repo, token, num)
    if pr is None or pr.get('draft') or pr.get('state') != 'open':
        return 'skipped'
    labels = [l['name'] for l in pr.get('labels') or []]
    if LABEL_HUMAIN in labels:
        print('[refresh] #%d : ignorée (%s)' % (num, LABEL_HUMAIN), flush=True)
        return 'skipped-humain'
    if pr.get('mergeable') is None:
        # Calcul GitHub en cours : une seconde chance avant de trancher.
        import time
        time.sleep(3)
        pr = pr_details(repo, token, num) or pr
    conflicts = pr.get('mergeable') is False and \
        (pr.get('mergeable_state') or '') == 'dirty'
    if conflicts:
        return mark_conflict(repo, token, pr, dry)
    behind = behind_by(repo, token, pr)
    if behind != 0:
        print('[refresh] #%d : en retard de %s' % (num, behind), flush=True)
        if dry:
            return 'dry-refresh'
        return try_refresh(repo, token, pr, dry)
    return mark_fresh(repo, token, pr, dry)


def main():
    dry = '--dry-run' in sys.argv
    token = os.environ['GH_TOKEN']
    repo = os.environ['GITHUB_REPOSITORY']
    if not valid_repo(repo):
        print('[refresh] GITHUB_REPOSITORY invalide : %r' % (repo,), flush=True)
        return
    event_name = os.environ.get('GITHUB_EVENT_NAME', '')
    with open(os.environ['GITHUB_EVENT_PATH']) as fh:
        event = json.load(fh)
    if event_name == 'pull_request':
        nums = [event['pull_request']['number']]
    else:
        only = os.environ.get('INPUT_PR_NUMBER') or \
            (event.get('inputs') or {}).get('pr_number')
        want = int(only) if (only and str(only).isdigit()) else None
        if only and want is None:
            print('[refresh] pr_number invalide : %r' % (only,), flush=True)
            return
        prs = list_open_prs(repo, token)
        nums = [p['number'] for p in prs
                if want is None or p['number'] == want]
    done = {}
    for num in nums:
        try:
            done[num] = handle_pr(repo, token, num, dry)
        except Exception as e:  # une PR ne bloque jamais les autres
            print('[refresh] #%s exception : %r' % (num, e), flush=True)
            done[num] = 'error'
    print('[refresh] terminé : %s' % (done,), flush=True)


if __name__ == '__main__':
    main()
