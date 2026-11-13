#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""Dispatch du résolveur de conflits depuis le gate Hermes Lite.

Portage minimal du dispatch de `hermes.yml` (dépôt duello) : sur VRAI conflit,
le gate ne se contente plus du label `hermes/conflit`, il envoie le workflow
`hermes-conflict-resolver.yml` rebaser + résoudre + pousser la PR.

Gardes (toutes portées de duello) :
- backoff croissant plafonné depuis le dernier rapport du résolveur ;
- verrou anti-doublon = check-run `Hermes sous-agent (conflits)` ouvert ;
- plafond de file (capacité des runners) + budget de dispatch par cycle ;
- JAMAIS d'état terminal : un échec sera retenté au cycle suivant.
"""

import datetime
import os
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))

from common import (LABEL_ESCALADE, LABEL_RESOLVER, RESOLVER_CHECK,  # noqa: E402
                    api, ensure_label, now, parse_utc, read_marker_int,
                    set_label)

RESOLVER_WORKFLOW = 'hermes-conflict-resolver.yml'
ATTEMPT_RE = r'<!-- hermes-resolver:attempts=(\d+) -->'
ATTEMPT_AT_RE = (r'<!-- hermes-resolver:at='
                 r'([0-9]{4}-[0-9]{2}-[0-9]{2}T[0-9]{2}:[0-9]{2}:[0-9]{2}Z) -->')

# Backoff min(15 * 2^n, 30) minutes depuis le dernier rapport (n = tentatives).
BACKOFF_BASE_MINUTES = 15
BACKOFF_MAX_MINUTES = 30
# Seuil purement INFORMATIF : au-delà, on informe un humain, on n'arrête rien.
MAX_ATTEMPTS_INFO = 3
# Un check ouvert plus vieux que ça ne peut plus être un run vivant.
CHECK_STALE_MINUTES = 60
# Frein de capacité : pas de nouveau dispatch si la file dépasse ce total.
QUEUE_MAX = 8

_CYCLE = {'dispatched': 0}


def max_per_cycle():
    """Budget de NOUVEAUX dispatchs par cycle (surchargable par env)."""
    try:
        return int(os.environ.get('HERMES_RESOLVER_MAX_PER_CYCLE', '3'))
    except ValueError:
        return 3


def backoff_minutes(attempts):
    """Délai avant le re-dispatch suivant : min(15 * 2^n, 30) minutes."""
    return min(BACKOFF_BASE_MINUTES * (2 ** max(int(attempts or 0), 0)),
               BACKOFF_MAX_MINUTES)


def last_report_at(repo, token, num):
    """Horodatage du dernier rapport du résolveur (None si aucun)."""
    import re
    best = None
    sc, comments = api('GET', '/repos/%s/issues/%d/comments?per_page=100'
                       % (repo, num), token=token)
    if sc == 200 and isinstance(comments, list):
        for comment in comments:
            found = re.search(ATTEMPT_AT_RE, comment.get('body') or '')
            if found:
                moment = parse_utc(found.group(1))
                if moment and (best is None or moment > best):
                    best = moment
    return best


def check_running(repo, token, sha):
    """Check-run `Hermes sous-agent (conflits)` ouvert et frais, ou None."""
    sc, data = api('GET', '/repos/%s/commits/%s/check-runs?per_page=100'
                   % (repo, sha), token=token)
    if sc != 200 or not isinstance(data, dict):
        return None
    for check in data.get('check_runs') or []:
        if check.get('name') != RESOLVER_CHECK:
            continue
        if check.get('status') == 'completed':
            continue
        started = parse_utc(check.get('started_at') or '')
        if started is None:
            return check
        age = (datetime.datetime.now(datetime.timezone.utc) - started)
        if age.total_seconds() < CHECK_STALE_MINUTES * 60:
            return check
    return None


def queue_depth(repo, token):
    """Runs du résolveur en attente ou en cours (frein de capacité)."""
    depth = 0
    for state in ('queued', 'in_progress'):
        sc, data = api(
            'GET', '/repos/%s/actions/workflows/%s/runs?status=%s&per_page=100'
            % (repo, RESOLVER_WORKFLOW, state), token=token)
        if sc == 200 and isinstance(data, dict):
            depth += len(data.get('workflow_runs') or [])
    return depth


def dispatch_resolver(repo, token, pr):
    """Envoie le résolveur sur la PR. Renvoie True si dispatché."""
    num = pr['number']
    if _CYCLE['dispatched'] >= max_per_cycle():
        print('[dispatch] #%d : budget du cycle atteint — reporté' % num,
              flush=True)
        return False
    head_ref = pr['head']['ref']
    head_sha = pr['head']['sha']
    attempts = read_marker_int(repo, token, num, ATTEMPT_RE)
    if attempts >= MAX_ATTEMPTS_INFO:
        print('[dispatch] #%d : %d tentatives — escalade informative'
              % (num, attempts), flush=True)
        set_label(pr, repo, token, LABEL_ESCALADE, True)
    delay = backoff_minutes(attempts)
    last = last_report_at(repo, token, num)
    if last is not None:
        waited = (datetime.datetime.now(datetime.timezone.utc) - last)
        if waited.total_seconds() < delay * 60:
            print('[dispatch] #%d : backoff %d min non écoulé — reporté'
                  % (num, delay), flush=True)
            return False
    running = check_running(repo, token, head_sha)
    if running:
        print('[dispatch] #%d : résolveur déjà en cours — pas de doublon'
              % num, flush=True)
        return False
    if queue_depth(repo, token) >= QUEUE_MAX:
        print('[dispatch] #%d : file saturée — reporté' % num, flush=True)
        return False
    ensure_label(repo, token, LABEL_RESOLVER, '8250df',
                 'Résolveur Hermes : rebase + résolution en cours')
    set_label(pr, repo, token, LABEL_RESOLVER, True)
    sc, _ = api('POST', '/repos/%s/check-runs' % repo, token=token, body={
        'name': RESOLVER_CHECK, 'head_sha': head_sha, 'status': 'in_progress',
        'started_at': now(),
        'output': {'title': 'Résolveur dispatché : rebase + résolution',
                   'summary': 'Le résolveur rebase `%s` sur `%s`, résout les '
                              'conflits, valide par cargo puis pousse.'
                              % (head_ref, pr['base']['ref'])}})
    if sc != 201:
        print('[dispatch] #%d : check-run non créé (%s) — verrou absent'
              % (num, sc), flush=True)
    sc, resp = api('POST', '/repos/%s/actions/workflows/%s/dispatches'
                   % (repo, RESOLVER_WORKFLOW), token=token, body={
                       'ref': 'main',
                       'inputs': {'pr_number': str(num), 'head_ref': head_ref,
                                  'base_ref': pr['base']['ref'],
                                  'head_sha': head_sha}})
    if sc != 204:
        print('[dispatch] #%d : échec HTTP %s : %s' % (num, sc, resp),
              flush=True)
        set_label(pr, repo, token, LABEL_RESOLVER, False)
        return False
    print('[dispatch] #%d : résolveur dispatché (%s → %s)'
          % (num, head_ref, pr['base']['ref']), flush=True)
    _CYCLE['dispatched'] += 1
    return True


def rearm_checks(repo, token, push_token, num, head_ref):
    """Re-arme la CI sur le nouveau head (un push de bot ne déclenche rien).

    Avec un PAT (HERMES_PUSH_TOKEN), le push déclenche les vrais événements :
    ré-armement inutile. Sinon dispatch `ci.yml` (best effort) et
    `hermes-lite.yml` (ré-évaluation du gate). Rend les workflows dispatchés.
    """
    if push_token:
        print('[rearm] PAT présent : événements réels, ré-armement inutile')
        return []
    done = []
    for workflow, ref, inputs in (
            ('ci.yml', head_ref, None),
            ('hermes-lite.yml', 'main', {'pr_number': str(num)})):
        body = {'ref': ref}
        if inputs:
            body['inputs'] = inputs
        sc, resp = api('POST', '/repos/%s/actions/workflows/%s/dispatches'
                       % (repo, workflow), token=token, body=body)
        if sc == 204:
            done.append(workflow)
            print('[rearm] %s dispatché' % workflow, flush=True)
        else:
            print('[rearm] %s : HTTP %s (%s)' % (workflow, sc, resp), flush=True)
    return done
