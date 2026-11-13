#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""Résolution des conflits : IA (fichier entier + hunk) et repli déterministe.

Portage minimal du cœur de `hermes-conflict-resolver.yml` (dépôt duello),
adapté à Crypte (Rust, pas de Node) :
- l'IA résout fichier par fichier, puis HUNK PAR HUNK au-delà de
  LLM_MAX_SIDE (aucune limite de taille de fichier) ;
- fichiers dérivés et binaires : JAMAIS soumis à l'IA, côté base + signalés ;
- sans clés LLM : mode 100 % déterministe (jamais d'échec silencieux).

Bibliothèque standard uniquement. Fonctions pures testables sans réseau :
`python3 resolve.py --selftest`.
"""

import json
import os
import urllib.error
import urllib.request

# Fichiers DÉRIVÉS : produits par un outil, ils se régénèrent, on ne les
# réécrit pas. `Cargo.lock` est le seul du dépôt (aucun `.generated.*`).
DERIVED_BASENAMES = ('cargo.lock',)

# Binaires : aucun merge textuel possible → côté base, jamais à l'IA.
# (`docs/bestiaire.png` et `assets/**` en font partie.)
BINARY_EXTS = ('.png', '.jpg', '.jpeg', '.gif', '.webp', '.pdf', '.mp4',
               '.mp3', '.ttf', '.otf', '.woff', '.woff2', '.ico', '.bin')

# Au-delà de ce côté, le mode « fichier entier » est écarté (bascule hunk).
LLM_MAX_SIDE = 12000
HUNK_CONTEXT_LINES = 30
HUNK_MAX_SIDE = 20000
HUNK_MAX_COUNT = 50

# Fichiers critiques : jamais de prise de côté — IA ou humain, sinon échec.
CRITICAL_HINTS = ('.github/', 'Cargo.toml', 'src/', 'deploy/', 'Dockerfile')

FW_URL = 'https://api.fireworks.ai/inference/v1/chat/completions'
FW_TIMEOUT = 120
# Même modèle que l'agent fw (DeepSeek V4.1 Flash via Fireworks).
FW_MODEL = 'accounts/fireworks/models/deepseek-v4p1-flash-1017'


def load_llm_config():
    """Lit la config LLM depuis l'environnement (secrets + vars du workflow)."""
    base = (os.environ.get('LLM_BASE') or '').strip().rstrip('/')
    return {'base': base,
            'key': (os.environ.get('LLM_KEY') or '').strip(),
            'model': (os.environ.get('LLM_MODEL') or '').strip()
                     or 'muse-spark-1.3-contributor',
            'fallback': (os.environ.get('LLM_MODEL_FALLBACK') or '').strip()
                        or 'combo/cheap',
            'fw_key': (os.environ.get('FIREWORKS_API_KEY') or '').strip()}


def llm_available(cfg):
    """Vrai si au moins une jambe LLM est configurée (passerelle ou fw)."""
    return bool(cfg['base'] or cfg['fw_key'])


def is_derived(path):
    """Vrai si le fichier est régénérable (jamais réécrit par l'IA)."""
    return os.path.basename(path).lower() in DERIVED_BASENAMES


def is_binary(path):
    """Vrai si le fichier est binaire (jamais soumis à l'IA)."""
    return path.lower().endswith(BINARY_EXTS)


def too_big_for_whole_file(*sides):
    """Vrai si un côté dépasse le mode « fichier entier » (→ hunk par hunk)."""
    return max([len(s or '') for s in sides] or [0]) > LLM_MAX_SIDE


def llm_skip_reason(path, *sides):
    """Raison d'écarter l'IA en mode fichier entier, ou None si elle peut."""
    if is_derived(path):
        return 'fichier dérivé — régénération, pas réécriture'
    if is_binary(path):
        return 'binaire — côté base, pas de merge textuel'
    biggest = max([len(s or '') for s in sides] or [0])
    if biggest > LLM_MAX_SIDE:
        return '%d car. > %d : mode fichier entier impossible' % (biggest,
                                                                  LLM_MAX_SIDE)
    return None


def llm_chat(cfg, sysmsg, user_text, label, max_tokens=8000, timeout=180):
    """Un appel LLM : passerelle locale d'abord, Fireworks direct en repli.

    Rend le texte dépouillé de son bloc de code, ou None si aucun appel n'a
    servi (clés absentes, HTTP KO, sortie refusée). Jamais d'exception.
    """
    legs = []
    if cfg['base']:
        legs.append((cfg['base'] + '/chat/completions', cfg['model'], 'router'))
        if cfg['fallback'] and cfg['fallback'] != cfg['model']:
            legs.append((cfg['base'] + '/chat/completions', cfg['fallback'],
                         'router'))
    if cfg['fw_key']:
        legs.append((FW_URL, FW_MODEL, 'fw'))
    for url, model, prov in legs:
        payload = {'model': model,
                   'messages': [{'role': 'system', 'content': sysmsg},
                                {'role': 'user', 'content': user_text}],
                   'max_tokens': max_tokens, 'temperature': 0.0}
        req = urllib.request.Request(url, data=json.dumps(payload).encode(),
                                     method='POST')
        if prov == 'fw':
            req.add_header('Authorization', 'Bearer ' + cfg['fw_key'])
        elif cfg['key']:
            req.add_header('Authorization', 'Bearer ' + cfg['key'])
        req.add_header('Content-Type', 'application/json')
        try:
            with urllib.request.urlopen(
                    req, timeout=(timeout if prov == 'router' else FW_TIMEOUT)
                    ) as rep:
                data = json.loads(rep.read())
        except urllib.error.HTTPError as err:
            print('[llm] %s (%s/%s) : HTTP %s' % (label, prov, model, err.code))
            continue
        except Exception as exc:
            print('[llm] %s (%s/%s) : %s' % (label, prov, model, exc))
            continue
        msg = (data.get('choices') or [{}])[0].get('message') or {}
        out = (msg.get('content') or msg.get('reasoning_content') or '').strip()
        if not out:
            print('[llm] %s (%s/%s) : réponse vide' % (label, prov, model))
            continue
        if out.startswith('```'):
            parts = out.split('```')
            out = parts[1] if len(parts) >= 2 else out
            out = out.split('\n', 1)[1] if '\n' in out else out
        if not out.strip() or any(m in out for m in ('<<<<<<<', '>>>>>>>')) \
                or '\n=======' in out:
            print('[llm] %s (%s/%s) : sortie refusée (marqueurs)'
                  % (label, prov, model))
            return None
        return out[:-1] if out.endswith('\n') else out
    return None


def resolve_file(cfg, path, base_t, ours_t, theirs_t):
    """Résout UN fichier entier par l'IA (None si écartée ou en échec)."""
    if not llm_available(cfg) or is_derived(path) or is_binary(path):
        return None
    if too_big_for_whole_file(base_t, ours_t, theirs_t):
        return None
    sysmsg = ('Tu es le résolveur Hermes (Crypte, TUI Rust) spécialisé dans la '
              'résolution de conflits de rebase. On te donne un fichier en '
              'conflit : BASE (ancêtre), A (branche de base, à conserver par '
              'défaut) et B (la PR). Produis le contenu final qui fusionne les '
              'intentions de A et B sans régression et SANS marqueurs. Réponds '
              'UNIQUEMENT avec le contenu complet dans un seul bloc de code.')
    user = ('FICHIER: %s\n\n===== BASE =====\n%s\n===== A (base) =====\n%s'
            '\n===== B (PR) =====\n%s') % (path, base_t, ours_t, theirs_t)
    return llm_chat(cfg, sysmsg, user, path)


def split_conflict_hunks(text):
    """Découpe un fichier marqué en segments.

    Rend (segments, n_hunks) ou (None, 0) si illisible. Segments : ("clean",
    texte) ou ("conflict", ours, theirs, base_ou_None). Styles merge et diff3.
    """
    lines = text.split('\n')
    segments, clean_buf = [], []
    i, n, hunks = 0, len(lines), 0
    while i < n:
        if lines[i].startswith('<<<<<<< '):
            if clean_buf:
                segments.append(('clean', '\n'.join(clean_buf)))
                clean_buf = []
            i += 1
            ours, theirs, base = [], [], None
            while i < n and not lines[i].startswith('=======') \
                    and not lines[i].startswith('||||||| '):
                ours.append(lines[i])
                i += 1
            if i < n and lines[i].startswith('||||||| '):
                base = []
                i += 1
                while i < n and not lines[i].startswith('======='):
                    base.append(lines[i])
                    i += 1
                base = '\n'.join(base)
            if i >= n:
                return None, 0
            i += 1
            while i < n and not lines[i].startswith('>>>>>>> '):
                theirs.append(lines[i])
                i += 1
            if i >= n:
                return None, 0
            i += 1
            segments.append(('conflict', '\n'.join(ours), '\n'.join(theirs), base))
            hunks += 1
        else:
            if lines[i].startswith(('=======', '>>>>>>> ', '||||||| ')):
                return None, 0
            clean_buf.append(lines[i])
            i += 1
    if clean_buf:
        segments.append(('clean', '\n'.join(clean_buf)))
    return segments, hunks


def resolve_hunk(cfg, path, idx, total, ctx_before, ctx_after, ours_h,
                 theirs_h, base_h):
    """Résout UN hunk par l'IA (contenu du hunk seul, ou None)."""
    sysmsg = ('Tu es le résolveur Hermes (Crypte) : on te donne UN SEUL hunk '
              'en conflit (A = base à conserver par défaut, B = PR), contexte '
              'en lecture seule. Produis le contenu final de CE HUNK UNIQUEMENT, '
              'sans marqueurs, sans reproduire le contexte, dans un bloc de code.')
    parts = ['FICHIER: %s (hunk %d/%d)' % (path, idx, total)]
    if base_h:
        parts.append('===== BASE (ce hunk) =====\n%s' % base_h)
    parts.append('===== A (base, ce hunk) =====\n%s' % ours_h)
    parts.append('===== B (PR, ce hunk) =====\n%s' % theirs_h)
    parts.append('===== CONTEXTE AVANT (ne pas reproduire) =====\n%s' % ctx_before)
    parts.append('===== CONTEXTE APRÈS (ne pas reproduire) =====\n%s' % ctx_after)
    return llm_chat(cfg, sysmsg, '\n'.join(parts),
                    '%s hunk %d/%d' % (path, idx, total))


def resolve_hunks(cfg, workdir, path):
    """Résolution hunk par hunk, sans limite de taille de fichier.

    Rend (contenu_complet, n_hunks) ou (None, 0)."""
    if is_derived(path) or is_binary(path) or not llm_available(cfg):
        return None, 0
    try:
        with open(os.path.join(workdir, path), encoding='utf-8') as fh:
            text = fh.read()
    except Exception as exc:
        print('[hunk] %s : lecture impossible (%s)' % (path, exc))
        return None, 0
    segments, n_hunks = split_conflict_hunks(text)
    if segments is None:
        print('[hunk] %s : marqueurs illisibles' % path)
        return None, 0
    if n_hunks == 0 or n_hunks > HUNK_MAX_COUNT:
        print('[hunk] %s : %d hunk(s) — abandon' % (path, n_hunks))
        return None, 0
    out_parts, prev_tail, idx = [], [], 0
    for pos, seg in enumerate(segments):
        if seg[0] == 'clean':
            out_parts.append(seg[1])
            prev_tail = seg[1].split('\n')[-HUNK_CONTEXT_LINES:]
            continue
        _, ours_h, theirs_h, base_h = seg
        idx += 1
        if max(len(ours_h), len(theirs_h), len(base_h or '')) > HUNK_MAX_SIDE:
            print('[hunk] %s hunk %d : trop gros — abandon' % (path, idx))
            return None, 0
        next_head = []
        for following in segments[pos + 1:]:
            if following[0] == 'clean':
                next_head = following[1].split('\n')[:HUNK_CONTEXT_LINES]
                break
        resolved = resolve_hunk(cfg, path, idx, n_hunks, '\n'.join(prev_tail),
                                '\n'.join(next_head), ours_h, theirs_h, base_h)
        if resolved is None:
            return None, 0
        out_parts.append(resolved)
        prev_tail = resolved.split('\n')[-HUNK_CONTEXT_LINES:]
    return '\n'.join(out_parts), n_hunks


def heuristic_resolve(path):
    """Repli déterministe : 'ours' / 'theirs' / 'union' / None (échec clair)."""
    low = path.lower()
    if is_derived(path):
        return 'ours'  # régénérable : côté base, signalé « à régénérer »
    if low.endswith('.lock'):
        return None  # autre lockfile : échec clair, jamais au hasard
    if is_binary(path):
        return 'ours'
    if any(h in path for h in CRITICAL_HINTS):
        return None  # code source, CI, manifeste : IA ou humain
    if low.endswith(('.md', '.txt', '.csv')):
        return 'union'
    return None


def valid_output(path, content, *sides):
    """Raison de REFUSER une sortie d'IA, ou None si utilisable.

    Contrôles précis, sans faux positif : marqueurs résiduels, troncature
    grossière, JSON/TOML réellement parsés. Les `.rs` ne sont pas analysés
    ici : `cargo` tranche sur l'arbre, et son échec déclenche le rattrapage
    déterministe (comme le typecheck jugeait les `.ts` chez duello).
    """
    if not content or not content.strip():
        return 'sortie vide'
    if any(m in content for m in ('<<<<<<<', '>>>>>>>')) or '\n=======' in content:
        return 'marqueurs de conflit résiduels'
    floor = int(min([len(s or '') for s in sides] or [0]) * 0.5)
    if floor > 0 and len(content) < floor:
        return 'sortie tronquée (%d car., minimum %d)' % (len(content), floor)
    low = path.lower()
    if low.endswith(('.json', '.jsonl')):
        for num, line in enumerate(content.splitlines(), 1):
            if not line.strip():
                continue
            try:
                json.loads(line)
            except Exception as exc:
                return 'JSON invalide ligne %d (%s)' % (num, exc)
    elif low.endswith('.toml'):
        try:
            import tomllib
        except ImportError:
            tomllib = None
        if tomllib is not None:
            try:
                tomllib.loads(content)
            except Exception as exc:
                return 'TOML invalide (%s)' % exc
    return None


def selftest():
    """Batterie locale sans réseau : `python3 resolve.py --selftest`."""
    fails = []

    def check(name, cond):
        print(('PASS ' if cond else 'FAIL ') + name)
        if not cond:
            fails.append(name)

    merge = 'a\n<<<<<<< HEAD\nours1\n=======\ntheirs1\n>>>>>>> x\nb\n'
    segs, n_h = split_conflict_hunks(merge)
    check('split merge : 1 hunk, 3 segments',
          n_h == 1 and segs is not None and len(segs) == 3
          and segs[1] == ('conflict', 'ours1', 'theirs1', None))
    diff3 = ('<<<<<<< HEAD\nA\n||||||| base\nB\n=======\nC\n>>>>>>> x\n')
    segs3, n3 = split_conflict_hunks(diff3)
    check('split diff3 : base lue',
          n3 == 1 and segs3[0] == ('conflict', 'A', 'C', 'B'))
    bad, n_bad = split_conflict_hunks('x\n=======\ny\n')
    check('split invalide : (None, 0)', bad is None and n_bad == 0)
    check('is_derived Cargo.lock', is_derived('Cargo.lock'))
    check('is_derived chemin quelconque', is_derived('crates/foo/Cargo.lock'))
    check('Cargo.toml non dérivé', not is_derived('Cargo.toml'))
    check('binaire png', is_binary('docs/bestiaire.png'))
    check('source non binaire', not is_binary('src/main.rs'))
    check('skip dérivé', llm_skip_reason('Cargo.lock', 'a', 'b', 'c') is not None)
    check('skip binaire',
          llm_skip_reason('a.png', 'a', 'b', 'c') is not None)
    check('skip gros côté',
          llm_skip_reason('f.rs', 'x' * 13000, '', '') is not None)
    check('IA permise petit .rs',
          llm_skip_reason('src/a.rs', 'a', 'b', 'c') is None)
    check('heuristique lock → ours', heuristic_resolve('Cargo.lock') == 'ours')
    check('heuristique png → ours',
          heuristic_resolve('assets/x.png') == 'ours')
    check('heuristique Cargo.toml → None',
          heuristic_resolve('Cargo.toml') is None)
    check('heuristique src → None',
          heuristic_resolve('src/app.rs') is None)
    check('heuristique md → union',
          heuristic_resolve('docs/NOTE.md') == 'union')
    check('valid vide', valid_output('a.rs', '   ', 'x') == 'sortie vide')
    check('valid marqueurs',
          valid_output('a.rs', 'a\n<<<<<<< x\n', 'aa') is not None)
    check('valid troncature',
          valid_output('a.rs', 'x', 'y' * 100) is not None)
    check('valid JSON ok',
          valid_output('a.json', '{"a": 1}\n', '{"a": 1}') is None)
    check('valid JSON ko',
          valid_output('a.json', '{"a": }\n', '{"a": 1}') is not None)
    cfg = {'base': '', 'key': '', 'model': 'm', 'fallback': 'm', 'fw_key': ''}
    check('sans clés : indisponible', not llm_available(cfg))
    check('jambe directe = Fireworks',
          FW_URL.startswith('https://api.fireworks.ai/')
          and FW_MODEL == 'accounts/fireworks/models/deepseek-v4p1-flash-1017')
    cfg_fw = dict(cfg, fw_key='x')
    check('clé fw : disponible', llm_available(cfg_fw))
    check('sans clés : resolve_file None',
          resolve_file(cfg, 'a.rs', 'a', 'b', 'c') is None)
    check('sans clés : resolve_hunks (None, 0)',
          resolve_hunks(cfg, '.', 'a.rs') == (None, 0))
    print('%d échec(s) sur le selftest' % len(fails))
    return 1 if fails else 0


if __name__ == '__main__':
    import sys
    if len(sys.argv) == 2 and sys.argv[1] == '--selftest':
        sys.exit(selftest())
    print('Usage : python3 resolve.py --selftest')
    sys.exit(2)
