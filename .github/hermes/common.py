#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""Outillage partagé du gate Hermes Lite et du résolveur de conflits.

Bibliothèque standard uniquement. Importé par `gate.py`, `dispatch.py` et
`resolver.py` (même dossier, via `sys.path`). Toute la logique réseau GitHub
vit ici : un seul `api()`, un seul masquage de secrets.
"""

import datetime
import json
import re
import urllib.error
import urllib.request

API = 'https://api.github.com'

# Marqueurs et labels partagés par le gate (dispatch) et le résolveur.
RESOLVER_CHECK = 'Hermes sous-agent (conflits)'
LABEL_RESOLVER = 'hermes/sous-agent-conflits'
LABEL_FAILED = 'hermes/resolution-echouee'
LABEL_ESCALADE = 'hermes/intervention-humaine'
RESOLVER_MARKER = '<!-- hermes-resolver:v1 -->'

_SECRETS = []


def arm_scrub(*secrets):
    """Mémorise les secrets à masquer dans les logs (jamais journalisés)."""
    del _SECRETS[:]
    _SECRETS.extend(s for s in secrets if s)


def scrub(text):
    """Remplace chaque secret connu par `***` (best effort, jamais d'exception)."""
    try:
        for secret in _SECRETS:
            if secret:
                text = text.replace(secret, '***')
    except Exception:
        pass
    return text


def now():
    """Horodatage UTC ISO-8601 (format attendu par l'API GitHub)."""
    return datetime.datetime.now(datetime.timezone.utc).strftime('%Y-%m-%dT%H:%M:%SZ')


def api(method, url, body=None, token=None):
    """Un appel API GitHub. Renvoie `(statut_http, objet_ou_texte)`."""
    data = json.dumps(body).encode() if body is not None else None
    req = urllib.request.Request(API + url, data=data, method=method)
    req.add_header('Authorization', 'Bearer ' + token)
    req.add_header('Accept', 'application/vnd.github+json')
    try:
        with urllib.request.urlopen(req, timeout=30) as rep:
            raw = rep.read()
            return rep.status, (json.loads(raw) if raw else None)
    except urllib.error.HTTPError as err:
        payload = err.read()
        try:
            return err.code, json.loads(payload)
        except Exception:
            return err.code, payload.decode('utf-8', errors='replace')


def ensure_label(repo, token, name, color, desc):
    """Crée le label s'il manque (422 s'il existe déjà : ignoré)."""
    api('POST', '/repos/%s/labels' % repo, token=token,
        body={'name': name, 'color': color, 'description': desc})


def set_label(pr, repo, token, name, add):
    """Pose ou retire un label (idempotent, lit les labels de l'objet PR)."""
    num = pr['number']
    has = name in [label['name'] for label in pr.get('labels') or []]
    if add and not has:
        api('POST', '/repos/%s/issues/%d/labels' % (repo, num),
            token=token, body={'labels': [name]})
    elif not add and has:
        api('DELETE', '/repos/%s/issues/%d/labels/%s' % (repo, num, name),
            token=token)


def post_or_update_comment(repo, token, num, marker, body):
    """Crée le commentaire `marker`, ou met à jour l'existant (un seul)."""
    sc, comments = api('GET', '/repos/%s/issues/%d/comments?per_page=100'
                       % (repo, num), token=token)
    if sc == 200 and isinstance(comments, list):
        for comment in comments:
            if marker in (comment.get('body') or ''):
                api('PATCH', '/repos/%s/issues/comments/%s' % (repo, comment['id']),
                    token=token, body={'body': body})
                return
    api('POST', '/repos/%s/issues/%d/comments' % (repo, num),
        token=token, body={'body': body})


def read_marker_int(repo, token, num, pattern):
    """Lit le plus grand entier `pattern` (un groupe) dans les commentaires."""
    best = 0
    sc, comments = api('GET', '/repos/%s/issues/%d/comments?per_page=100'
                       % (repo, num), token=token)
    if sc == 200 and isinstance(comments, list):
        for comment in comments:
            found = re.search(pattern, comment.get('body') or '')
            if found:
                best = max(best, int(found.group(1)))
    return best


def parse_utc(text):
    """Horodatage UTC ISO-8601 → datetime, ou None si illisible."""
    try:
        return datetime.datetime.strptime(text, '%Y-%m-%dT%H:%M:%SZ').replace(
            tzinfo=datetime.timezone.utc)
    except (TypeError, ValueError):
        return None
