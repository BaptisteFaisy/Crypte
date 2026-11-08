#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""Balayeur de la file Mergify — remet les PR ejectees dans la file, tout seul.

Constat (PR #78, nov 2026) : quand Mergify sort une PR de sa file sur un
echec, il ne la reprend jamais de lui-meme, meme CI verte : neuf heures
immobile, alors qu'un `@mergifyio queue` suffit. Ce balayeur ferme la boucle :
toute PR prete (checks verts, fusionnable) hors file y est remise, 2 fois max
par tete puis escalade humaine.

Pas de rebase ici : la file fusionne en squash et met les PR a jour par merge
de main (decision user 2026-11-08, `.mergify.yml`) — les commits de fusion
dans une branche sont normaux, jamais de reecriture. Seuls les vrais conflits
(`dirty`) demandent une main humaine, avec la marche a suivre en commentaire.

Declencheurs (file-mergify.yml) : cron 10 min (filet), push sur main, fin de
check, commentaire Mergify, manuel. Concurrency unique sans annulation.
Secret `AUTOMERGE_PAT` (fin : contenus + demandes + tickets en ecriture) :
signe les seules commandes Mergify (un ordre poste par un bot est ignore).
Guidance, labels et lectures passent par le jeton de l'installation.
Sans PAT : diagnostic + marche a suivre seulement.
Stdlib uniquement.
"""

import json
import os
import re
import time

API = 'https://api.github.com'
REQUIRED = ('ci-budget', 'stale-base-guard', 'hermes-lite/verdict')
LABEL_CONFLIT = 'mergify/conflit'
LABEL_ESCALADE = 'mergify/intervention'
MAX_REQUEUE = 2
REQUEUE_COOLDOWN_MIN = 15
BASE = 'main'

DRY = os.environ.get('SWEEP_DRY_RUN') == '1'


def api(method, url, body=None, token=None):
    import urllib.request as _ur
    import urllib.error as _ue
    data = json.dumps(body).encode() if body is not None else None
    req = _ur.Request(API + url, data=data, method=method)
    req.add_header('Authorization', 'Bearer ' + token)
    req.add_header('Accept', 'application/vnd.github+json')
    try:
        with _ur.urlopen(req, timeout=30) as resp:
            raw = resp.read()
            return resp.status, (json.loads(raw) if raw else None)
    except _ue.HTTPError as err:
        payload = err.read()
        try:
            return err.code, json.loads(payload)
        except Exception:
            return err.code, payload.decode('utf-8', errors='replace')


def ensure_label(repo, token, name, color, desc):
    if DRY:
        print('[dry] assurer label %s' % name)
        return
    api('POST', '/repos/%s/labels' % repo, token=token,
        body={'name': name, 'color': color, 'description': desc})


def set_label(pr, repo, token, name, add):
    num = pr['number']
    have = [label['name'] for label in pr.get('labels') or []]
    if add == (name in have):
        return
    if DRY:
        print('[dry] #%d label %s %s' % (num, name, '+' if add else '-'))
        return
    if add:
        api('POST', '/repos/%s/issues/%d/labels' % (repo, num),
            token=token, body={'labels': [name]})
    else:
        api('DELETE', '/repos/%s/issues/%d/labels/%s' % (repo, num, name),
            token=token)


def post_comment(repo, token, num, body):
    if DRY:
        print('[dry] #%d commentaire : %s' % (num, body.splitlines()[0]))
        return 0
    code, resp = api('POST', '/repos/%s/issues/%d/comments' % (repo, num),
                     token=token, body={'body': body})
    if code != 201:
        print('[sweep] #%d commentaire %s : %s' % (num, code, resp))
        return None
    return (resp or {}).get('id')


def list_comments(repo, token, num):
    out, page = [], 1
    while True:
        code, items = api('GET', '/repos/%s/issues/%d/comments'
                          '?per_page=100&page=%d' % (repo, num, page),
                          token=token)
        if code != 200 or not isinstance(items, list):
            break
        out += items
        if len(items) < 100:
            break
        page += 1
    return out


def list_open_prs(repo, token):
    out, page = [], 1
    while True:
        code, items = api('GET', '/repos/%s/pulls?state=open&per_page=100'
                          '&page=%d' % (repo, page), token=token)
        if code != 200 or not isinstance(items, list):
            break
        out += items
        if len(items) < 100:
            break
        page += 1
    return out


def get_pr(repo, token, num):
    code, pr = api('GET', '/repos/%s/pulls/%d' % (repo, num), token=token)
    return pr if code == 200 and isinstance(pr, dict) else None


def last_mergify(comments):
    """Dernier verdict Mergify : (etat, sha courte) ou None."""
    bodies = [c.get('body') or '' for c in comments
              if 'Mergify Payload' in (c.get('body') or '')]
    if not bodies:
        return None
    body = bodies[-1]
    state = re.search(r'"state":\s*"(\w+)"', body)
    sha = re.search(r'(?:Left|Entered) the queue.*?· `?([0-9a-f]{7,40})`?',
                    body)
    return {'state': state.group(1) if state else '',
            'sha': sha.group(1) if sha else ''}


def check_runs(repo, token, sha):
    code, data = api('GET', '/repos/%s/commits/%s/check-runs?per_page=100'
                     % (repo, sha), token=token)
    return (data or {}).get('check_runs') or []


def queued_now(repo, token, sha, mergify):
    """La PR est-elle dans la file en ce moment ?"""
    if mergify and mergify['state'] == 'queued' and mergify['sha'] \
            and sha.startswith(mergify['sha']):
        return True
    for run in check_runs(repo, token, sha):
        if run.get('name') == 'Mergify Merge Queue' \
                and run.get('status') in ('queued', 'in_progress',
                                          'waiting', 'requested', 'pending'):
            return True
    return False


def marker_count(comments, action, sha):
    tag = '<!-- crypte-file:%s sha=%s' % (action, sha)
    return sum(1 for c in comments if tag in (c.get('body') or ''))


def marker_age_min(comments, action, sha):
    """Age du dernier marqueur (minutes), None si absent."""
    import datetime as _dt
    tag = '<!-- crypte-file:%s sha=%s' % (action, sha)
    dates = [c.get('created_at') or '' for c in comments
             if tag in (c.get('body') or '')]
    if not dates:
        return None
    try:
        when = _dt.datetime.fromisoformat(dates[-1].replace('Z', '+00:00'))
        now = _dt.datetime.now(_dt.timezone.utc)
        return max(0, int((now - when).total_seconds() // 60))
    except ValueError:
        return 999


def checks_green(repo, token, sha):
    """Les checks requis sont-ils tous verts sur cette tete ?"""
    latest = {}
    for run in check_runs(repo, token, sha):
        name = run.get('name')
        if run.get('id', 0) > latest.get(name, {}).get('id', -1):
            latest[name] = run
    detail = []
    for name in REQUIRED:
        run = latest.get(name)
        if run is None:
            detail.append('%s=?' % name)
        else:
            detail.append('%s=%s/%s' % (name, run.get('status'),
                                        run.get('conclusion') or '?'))
    ok = all((latest.get(name) or {}).get('status') == 'completed' and
             (latest.get(name) or {}).get('conclusion') == 'success'
             for name in REQUIRED)
    return ok, ','.join(detail)


def handle_checks(repo, tok, pr, comments, sha, detail):
    num = pr['number']
    parts = dict(item.split('=', 1) for item in detail.split(','))
    bad = sorted(n for n, v in parts.items()
                 if v.startswith('completed/') and not v.endswith('/success'))
    if bad:
        marker = '<!-- crypte-file:checks-rouges sha=%s -->' % sha
        body = ('CI non verte sur cette tete : `%s` (auteur : reparer puis '
                'pousser). La file reprendra ensuite (ou `@mergifyio '
                'queue`).' % ', '.join(bad))
        return issue(post_once(repo, tok, num, comments, marker, body),
                     'guide:checks-rouges', 'attente:checks-rouges-connus')
    missing = sorted(n for n, v in parts.items() if v == '?')
    if missing:
        marker = '<!-- crypte-file:ci-absente sha=%s -->' % sha
        body = ('Aucune CI sur cette tete (`%s` sans verdict : vieux push ou '
                'CI muette). Relancez en poussant - ex. `git commit '
                '--allow-empty -m "ci: relance" && git push` - la file '
                'reprendra ensuite.' % ', '.join(missing))
        return issue(post_once(repo, tok, num, comments, marker, body),
                     'guide:ci-absente', 'attente:ci-absente-connue')
    return 'attente:checks(%s)' % detail


def post_once(repo, token, num, comments, marker, body):
    """'poste', 'connu' (marqueur deja la) ou 'refuse' (API en echec)."""
    if any(marker in (c.get('body') or '') for c in comments):
        return 'connu'
    if post_comment(repo, token, num, marker + '\n' + body) is None:
        return 'refuse'
    return 'poste'


def issue(post_result, guide, connu):
    """Verdict d'un commentaire de guidance : poste / connu / refuse."""
    if post_result == 'poste':
        return guide
    if post_result == 'connu':
        return connu
    return 'erreur:commentaire'


MERGE_CMDS = ('```\ngit fetch origin && git checkout <branche> && '
              'git merge origin/main  # resoudre, puis\n'
              'git push\n```')


def handle_dirty(repo, tok, pr, comments, sha):
    marker = '<!-- crypte-file:conflit sha=%s -->' % sha
    body = ('Conflits avec `%s` : resolution manuelle requise (par merge, '
            'la file squash ne rebase jamais).\n%s\nLa file reprendra '
            'ensuite (ou `@mergifyio queue`).' % (BASE, MERGE_CMDS))
    res = post_once(repo, tok, pr['number'], comments, marker, body)
    if res == 'poste':
        set_label(pr, repo, tok, LABEL_CONFLIT, True)
    return issue(res, 'guide:conflit', 'attente:conflit-connu')


def handle_queue(repo, rtok, wtok, pat, pr, comments, sha, mergify):
    num = pr['number']
    if queued_now(repo, rtok, sha, mergify):
        set_label(pr, repo, rtok, LABEL_CONFLIT, False)
        return 'file:dedans'
    done = marker_count(comments, 'requeue', sha)
    if done >= MAX_REQUEUE:
        marker = '<!-- crypte-file:escalade sha=%s -->' % sha
        body = ('%d remises en file sans succes sur cette tete - main '
                'humaine requise (voir les verdicts Mergify ci-dessus).'
                % MAX_REQUEUE)
        res = post_once(repo, rtok, num, comments, marker, body)
        if res == 'poste':
            set_label(pr, repo, rtok, LABEL_ESCALADE, True)
            return 'escalade:nouvelle'
        return issue(res, 'escalade:nouvelle', 'escalade:connue')
    age = marker_age_min(comments, 'requeue', sha)
    if age is not None and age < REQUEUE_COOLDOWN_MIN:
        return 'attente:antirebond(%dmin)' % age
    if not pat:
        marker = '<!-- crypte-file:file-sans-pat sha=%s -->' % sha
        return issue(post_once(repo, rtok, num, comments, marker,
                               'PR prete (checks verts) mais hors file, et '
                               'sans secret `AUTOMERGE_PAT` je ne peux pas '
                               'commander Mergify : postez `@mergifyio '
                               'queue` pour la remettre en file.'),
                     'guide:file-sans-pat', 'attente:file-sans-pat-connue')
    if DRY:
        return 'dry:remettrait'
    if post_comment(repo, wtok, num,
                    '@mergifyio queue\n'
                    '<!-- crypte-file:requeue sha=%s n=%d -->\n'
                    'Remise en file automatique (balayeur, tentative %d/%d).'
                    % (sha, done + 1, done + 1, MAX_REQUEUE)) is None:
        return 'erreur:commande-refusee'
    set_label(pr, repo, rtok, LABEL_CONFLIT, False)
    return 'file:remise(%d)' % (done + 1)


def handle_pr(repo, rtok, wtok, pat, num):
    pr = get_pr(repo, rtok, num)
    if pr is None or pr.get('state') != 'open' or pr.get('draft'):
        return 'ignore:fermee-ou-brouillon'
    if (pr.get('base') or {}).get('ref') != BASE:
        return 'ignore:base'
    if 'hold' in [label['name'] for label in pr.get('labels') or []]:
        return 'ignore:hold'
    if (pr.get('head') or {}).get('repo', {}).get('fork'):
        return 'ignore:fork'
    if pr.get('mergeable') is None:
        time.sleep(5)  # GitHub calcule mergeable en paresseux
        pr = get_pr(repo, rtok, num)
        if pr is None or pr.get('mergeable') is None:
            return 'attente:mergeable-inconnu'
    sha = pr['head']['sha']
    comments = list_comments(repo, rtok, pr['number'])
    if pr.get('mergeable') is False and \
            (pr.get('mergeable_state') or '') == 'dirty':
        return handle_dirty(repo, rtok, pr, comments, sha)
    ok, detail = checks_green(repo, rtok, sha)
    if not ok:
        return handle_checks(repo, rtok, pr, comments, sha, detail)
    if pr.get('mergeable') is not True:
        return 'attente:non-fusionnable'
    return handle_queue(repo, rtok, wtok, pat, pr, comments, sha,
                        last_mergify(comments))


def prs_for_event(repo, token, event_name, event):
    if event_name == 'issue_comment':
        user = (event.get('comment') or {}).get('user') or {}
        if user.get('login') != 'mergify[bot]':
            return None
    if event_name == 'workflow_dispatch':
        only = os.environ.get('INPUT_PR_NUMBER', '').strip()
        if only.isdigit():
            return [int(only)]
    # Toujours complet : en rafale GitHub annule les runs en attente et seul
    # le dernier survit — un perimetre cible ferait rater les autres PR.
    # A cette echelle (quelques PR ouvertes), le cout est negligeable.
    return [p['number'] for p in list_open_prs(repo, token)]


def main():
    rtok = os.environ['GH_TOKEN']
    pat = os.environ.get('SWEEP_PAT', '')
    wtok = pat or rtok
    repo = os.environ['GITHUB_REPOSITORY']
    event_name = os.environ.get('GITHUB_EVENT_NAME', '')
    with open(os.environ['GITHUB_EVENT_PATH']) as handle:
        event = json.load(handle)
    if not pat:
        print('AUTOMERGE_PAT absent : mode diagnostic (pas de commande)')
    nums = prs_for_event(repo, rtok, event_name, event)
    if nums is None:
        print('commentaire non-Mergify, rien a faire')
        return
    ensure_label(repo, rtok, LABEL_CONFLIT, 'd73a4a', 'Pose par file-mergify')
    ensure_label(repo, rtok, LABEL_ESCALADE, 'fbca04', 'Pose par file-mergify')
    results = []
    for num in nums:
        try:
            res = handle_pr(repo, rtok, wtok, pat, num)
        except Exception as exc:
            res = 'erreur:exception(%r)' % exc
        print('#%d : %s' % (num, res))
        results.append(res)
    print('[sweep] termine : %d PR, %s' % (len(nums), results))


if __name__ == '__main__':
    main()
