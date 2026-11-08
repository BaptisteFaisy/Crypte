#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""Gate Hermes Lite — fraîcheur exigée AU MOMENT DU MERGE ([HERMES-REFRESH], porté du gate `duello`).

Règles :
- retard sans conflit  → mise à jour serveur (PUT /pulls/N/update-branch,
  méthode UPDATE_METHOD, merge de la base par défaut) ; ré-évaluée dans ce run.
- vrai conflit         → check rouge + label `hermes/conflit` + DISPATCH du
  résolveur (`dispatch.py` → workflow `hermes-conflict-resolver.yml` : rebase
  + résolution IA + validation cargo + push) ; escalade humaine si irrésoluble.
- frais + checks requis verts → merge (MERGE_METHOD, squash par défaut),
  une PR à la fois.

Méthodes alignées sur .mergify.yml : jamais de rebase, qui refuse les PR
ayant fusionné main en elles (constaté 2026-11-08, PR #73).

En dry-run (HERMES_LITE_DRY_RUN=1), le gate évalue et étiquette mais ne
dispatche PAS le résolveur (un dispatch pousse sur la branche : écriture).
"""

CHECK_NAME = 'hermes-lite/verdict'
LABEL_OK = 'hermes/approuvee'
LABEL_CONFLIT = 'hermes/conflit'

import json
import os
import sys
import time

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))

from common import api, ensure_label, set_label  # noqa: E402
from dispatch import dispatch_resolver  # noqa: E402

# Méthodes GitHub (env du workflow hermes-lite.yml, alignées sur .mergify.yml).
MERGE_METHOD = os.environ.get('MERGE_METHOD') or 'squash'
UPDATE_METHOD = os.environ.get('UPDATE_METHOD') or 'merge'


def handle_conflict(pr, repo, token, base_ref, sha, dry, detail):
    """Vrai conflit : check rouge + label + dispatch du résolveur (sauf dry).

    Le dispatch pousse sur la branche : en dry-run, on l'annonce sans le faire.
    Jamais d'état terminal : backoff et re-dispatch gérés côté `dispatch.py`.
    """
    num = pr['number']
    dispatched = False if dry else dispatch_resolver(repo, token, pr)
    if dispatched:
        summary = ('Résolveur Hermes dispatché : rebase + résolution auto + '
                   'validation cargo + push. ' + detail)
    elif dry:
        summary = ('Dry-run : le résolveur serait dispatché ici. ' + detail
                   + ' Rebase manuel puis push en attendant.')
    else:
        summary = ('Résolveur non dispatché (backoff, file saturée ou déjà en '
                   'cours — voir logs ; retenté au cycle suivant). ' + detail)
    create_check(repo, token, sha, False,
                 'Conflits avec %s — résolution auto' % base_ref, summary)
    set_label(pr, repo, token, LABEL_CONFLIT, True)
    set_label(pr, repo, token, LABEL_OK, False)
    print('[gate] #%d : conflit → dispatch %s'
          % (num, 'OUI' if dispatched else 'non'), flush=True)
    return 'conflict'


def refresh_branch(repo, token, pr):
    """Mise à jour serveur d'une PR en retard (UPDATE_METHOD). Renvoie
    'refreshed'/'conflict'/'error'."""
    num = pr['number']
    expected = pr['head']['sha']
    sc, resp = api('PUT', '/repos/%s/pulls/%d/update-branch' % (repo, num),
                   token=token,
                   body={'update_method': UPDATE_METHOD, 'expected_head_sha': expected})
    if sc == 202:
        return 'refreshed'
    if sc in (409, 422):
        return 'conflict'
    print('[gate] #%d update-branch %s : %s' % (num, sc, resp), flush=True)
    return 'error'


def create_check(repo, token, sha, ok, title, summary):
    api('POST', '/repos/%s/check-runs' % repo, token=token, body={
        'name': CHECK_NAME,
        'head_sha': sha,
        'status': 'completed',
        'conclusion': 'success' if ok else 'failure',
        'output': {'title': title, 'summary': summary},
    })


def required_checks_green(repo, token, pr, required):
    """Statuts de commit requis verts. Liste vide => considéré vert."""
    if not required:
        return True
    sha = pr['head']['sha']
    sc, data = api('GET', '/repos/%s/commits/%s/status' % (repo, sha), token=token)
    if sc != 200 or not data:
        return False
    states = {s['context']: s['state'] for s in data.get('statuses') or []}
    for ctx in required:
        if states.get(ctx) != 'success':
            return False
    return True


def settle_after_refresh(repo, token, num, required, attempts=10, pause=6, dry=False):
    """[HERMES-REFRESH] Ré-évalue la PR après la mise à jour serveur, DANS CE RUN.

    Le push effectué par `update-branch` est un push de bot : il ne déclenche
    AUCUN workflow (anti-récursion GitHub), et hermes-lite n'a ni cron ni
    dispatch possible avec GITHUB_TOKEN. Sans cette boucle, une PR rebasée
    resterait ouverte pour toujours : personne ne la re-évaluerait.
    Pollue borné (~60 s), puis rend la main (la veille événementielle
    rattrapera au prochain check_run de ce head).
    """
    for i in range(attempts):
        time.sleep(pause if i else 2)
        sc, pr = api('GET', '/repos/%s/pulls/%d' % (repo, num), token=token)
        if sc != 200 or not isinstance(pr, dict):
            continue
        if pr.get('merged'):
            return 'merged'
        sha = pr['head']['sha']
        base_ref = pr['base']['ref']
        sc, cmp_ = api('GET', '/repos/%s/compare/%s...%s' % (repo, base_ref, sha),
                       token=token)
        behind = cmp_.get('behind_by') if sc == 200 else -1
        behind = behind if isinstance(behind, int) else -1
        if behind > 0:
            continue  # mise à jour pas encore appliquée (ou main a bougé entre-temps)
        conflicts = (pr.get('mergeable') is False
                     and (pr.get('mergeable_state') or '') == 'dirty')
        if conflicts:
            return handle_conflict(
                pr, repo, token, base_ref, sha, dry,
                'La mise à jour serveur a révélé un conflit réel.')
        set_label(pr, repo, token, LABEL_OK, True)
        create_check(repo, token, sha, True, 'PR à jour — merge par le gate',
                     'Fraîche au moment du merge (settle après mise à jour serveur).')
        if dry:
            return 'dry'
        m = try_merge(repo, token, pr, required)
        print('[gate] #%d : settle merge → %s' % (num, m), flush=True)
        if m != 'waiting':
            return m
    return 'timeout'


def try_merge(repo, token, pr, required):
    """Merge la PR (MERGE_METHOD) si fraîche et checks requis verts."""
    num = pr['number']
    sha = pr['head']['sha']
    # Re-vérification de fraîcheur au dernier moment (défense anti-course)
    sc, cmp_ = api('GET', '/repos/%s/compare/%s...%s'
                   % (repo, pr['base']['ref'], sha), token=token)
    if sc == 200 and cmp_.get('behind_by') not in (0, None):
        return 'behind'
    if not required_checks_green(repo, token, pr, required):
        return 'waiting'
    sc, resp = api('PUT', '/repos/%s/pulls/%d/merge' % (repo, num), token=token,
                   body={'sha': sha, 'merge_method': MERGE_METHOD})
    if sc == 200:
        return 'merged'
    if sc == 409:
        return 'behind'
    print('[gate] #%d merge %s : %s' % (num, sc, resp), flush=True)
    return 'error'


def handle_pr(pr, repo, token, required, dry=False):
    num = pr['number']
    sha = pr['head']['sha']
    base_ref = pr['base']['ref']
    mergeable = pr.get('mergeable')           # None = calcul en cours
    mergeable_state = pr.get('mergeable_state') or ''

    sc, cmp_ = api('GET', '/repos/%s/compare/%s...%s' % (repo, base_ref, sha),
                   token=token)
    behind = cmp_.get('behind_by') if sc == 200 else -1
    behind = behind if isinstance(behind, int) else -1
    fresh = behind == 0
    conflicts = (mergeable is False and mergeable_state == 'dirty')

    if conflicts:
        return handle_conflict(
            pr, repo, token, base_ref, sha, dry,
            'En dernier recours : fusionner main, résoudre puis push.')
    set_label(pr, repo, token, LABEL_CONFLIT, False)

    if not fresh:
        if dry:
            print('[gate] #%d : en retard de %s commit(s) — mise à jour serveur (dry-run)' % (num, behind))
            return 'dry'
        r = refresh_branch(repo, token, pr)
        print('[gate] #%d : en retard de %s → refresh %s' % (num, behind, r), flush=True)
        if r == 'refreshed':
            # Le push de la mise à jour est un push de bot : aucun workflow ne se
            # déclenche. Ré-évaluation et merge DANS CE RUN.
            return settle_after_refresh(repo, token, num, required, dry=dry)
        if r == 'conflict':
            # La mise à jour serveur a été refusée : conflit réel après tout.
            return handle_conflict(
                pr, repo, token, base_ref, sha, dry,
                'La mise à jour serveur a été refusée (conflit réel).')
        return r

    set_label(pr, repo, token, LABEL_OK, True)
    create_check(repo, token, sha, True,
                 'PR à jour — merge par le gate',
                 'Fraîche au moment du merge ; %s' %
                 ('checks requis verts' if required else 'aucun check requis configuré'))
    if dry:
        print('[gate] #%d : fraîche — merge possible (dry-run)' % num)
        return 'dry'
    m = try_merge(repo, token, pr, required)
    print('[gate] #%d : merge → %s' % (num, m), flush=True)
    return m


def list_open_prs(repo, token):
    prs, page = [], 1
    while True:
        sc, plist = api('GET', '/repos/%s/pulls?state=open&per_page=100&page=%d'
                        % (repo, page), token=token)
        if sc != 200 or not isinstance(plist, list):
            break
        prs += plist
        if len(plist) < 100:
            break
        page += 1
    return prs


def main():
    import os
    token = os.environ['GH_TOKEN']
    repo = os.environ['GITHUB_REPOSITORY']
    event_name = os.environ['GITHUB_EVENT_NAME']
    required = [s.strip() for s in
                os.environ.get('HERMES_REQUIRED_CHECKS', '').split(',') if s.strip()]
    dry = os.environ.get('HERMES_LITE_DRY_RUN') == '1'

    for lbl in (LABEL_OK, LABEL_CONFLIT):
        ensure_label(repo, token, lbl, '0e8a16' if lbl == LABEL_OK else 'd73a4a',
                     'Posé par hermes-lite')

    with open(os.environ['GITHUB_EVENT_PATH']) as fh:
        event = json.load(fh)

    if event_name == 'workflow_dispatch':
        only = (event.get('inputs') or {}).get('pr_number') or None
        prs = list_open_prs(repo, token)
        if only:
            prs = [p for p in prs if p['number'] == int(only)]
    elif event_name == 'pull_request':
        prs = [event['pull_request']]
    elif event_name == 'check_run':
        # Un check vient de finir : ré-évalue les PR dont le head porte ce check.
        sha = event['check_run']['head_sha']
        prs = [p for p in list_open_prs(repo, token)
               if p['head']['sha'] == sha]
    else:
        prs = []

    merged = 0
    for pr in prs:
        if pr.get('draft') or pr.get('state') != 'open':
            continue
        try:
            r = handle_pr(pr, repo, token, required, dry)
            if r == 'merged':
                merged += 1
        except Exception as e:
            print('[gate] #%s exception : %r' % (pr.get('number'), e), flush=True)
    print('[gate] terminé : %d PR traitée(s), %d merge(s)' % (len(prs), merged),
          flush=True)


if __name__ == '__main__':
    main()
