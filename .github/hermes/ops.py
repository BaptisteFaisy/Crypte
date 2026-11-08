#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""Opérations git + cargo du résolveur Hermes (stdlib, sans état global).

Extrait de `resolver.py` (règle 500 lignes / 30 fonctions). Le workdir est
passé en paramètre : aucune globale, fonctions testables. Les logs passent
par `common.scrub` (secrets armés par l'appelant).
"""

import os
import shutil
import subprocess
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))

from common import scrub  # noqa: E402


def run(cmd, timeout=300, cwd=None):
    """Exécute une commande, journalise (scrubée), rend le CompletedProcess."""
    print('+ ' + scrub(' '.join(cmd)), flush=True)
    try:
        proc = subprocess.run(cmd, capture_output=True, text=True,
                              errors='replace', timeout=timeout, cwd=cwd,
                              env={**os.environ, 'GIT_EDITOR': 'true',
                                   'GIT_PAGER': 'cat'})
    except subprocess.TimeoutExpired:
        print('!! timeout après %ss' % timeout, flush=True)
        return None
    if proc.stdout and proc.stdout.strip():
        print(scrub(proc.stdout[-2500:]), flush=True)
    if proc.returncode != 0 and proc.stderr and proc.stderr.strip():
        print(scrub(proc.stderr[-2500:]), flush=True)
    return proc


def git(work, *args, timeout=300):
    """`git ...` dans `work` (None = workdir courant). Jamais de `-c` seul."""
    return run(['git'] + list(args), timeout=timeout, cwd=work)


def git_ok(work, *args):
    """Vrai si `git ...` réussit (None sur timeout = échec, pas de crash)."""
    proc = git(work, *args)
    return proc is not None and proc.returncode == 0


def file_content(work, rev, path):
    """Contenu de `path` à la révision `rev` ('' si absent)."""
    proc = run(['git', 'show', '%s:%s' % (rev, path)], timeout=60, cwd=work)
    return proc.stdout if proc and proc.returncode == 0 else ''


def apply_mode(work, path, mode):
    """Applique un mode déterministe (pendant un rebase, --ours = la BASE)."""
    if mode == 'ours':
        return git_ok(work, 'checkout', '--ours', '--', path)
    if mode == 'theirs':
        return git_ok(work, 'checkout', '--theirs', '--', path)
    if mode == 'union':
        ours = run(['git', 'show', 'HEAD:' + path], timeout=60, cwd=work)
        theirs = run(['git', 'show', 'REBASE_HEAD:' + path], timeout=60, cwd=work)
        if ours is None or theirs is None:
            return False
        merged = sorted({line.rstrip('\r')
                         for line in (ours.stdout + '\n' + theirs.stdout)
                         .splitlines() if line.strip()})
        with open(os.path.join(work, path), 'w', encoding='utf-8', newline='\n') as fh:
            fh.write('\n'.join(merged) + '\n')
        return True
    return False


def fetch_base(work, base_ref, head_ref):
    """Ramène origin/<base> au plus récent et rend son SHA."""
    git(work, 'fetch', '--no-tags', 'origin', base_ref, head_ref, timeout=300)
    proc = run(['git', 'rev-parse', 'origin/' + base_ref], timeout=60, cwd=work)
    return proc.stdout.strip() if proc and proc.returncode == 0 else ''


def patch_ids(work, revrange):
    """{patch-id} des commits non-merges de la plage, ou None si indéterminable."""
    log = git(work, 'log', '-p', '--no-merges', '--pretty=format:commit %H',
              revrange, timeout=300)
    if log is None or log.returncode != 0:
        return None
    pid = subprocess.run(['git', 'patch-id', '--stable'], input=log.stdout,
                         capture_output=True, text=True, errors='replace',
                         cwd=work)
    if pid.returncode != 0:
        return None
    return {line.split()[0] for line in pid.stdout.splitlines() if line.strip()}


def cargo_validate(work):
    """Validation cargo de l'arbre. Rend (ok, notes)."""
    notes = []
    if shutil.which('cargo') is None:
        notes.append('Cargo indisponible sur le runner — la CI tranchera après push.')
        return True, notes
    steps = [('fmt', ['cargo', 'fmt', '--check'], 120),
             ('clippy', ['cargo', 'clippy', '--all-targets', '--',
                         '-D', 'warnings'], 280),
             ('build', ['cargo', 'build'], 280),
             ('test', ['cargo', 'test'], 280)]
    for name, cmd, timeout in steps:
        proc = run(cmd, timeout=timeout, cwd=work)
        if proc is None or proc.returncode != 0:
            tail = ((proc.stdout or '') + (proc.stderr or ''))[-2000:] \
                if proc else 'timeout'
            notes.append('`%s` ❌ :\n```\n%s\n```' % (' '.join(cmd), tail))
            print('[cargo] %s : ÉCHEC' % name, flush=True)
            return False, notes
        notes.append('`%s` ✅.' % ' '.join(cmd))
    print('[cargo] arbre vert (fmt + clippy + build + test)', flush=True)
    return True, notes
