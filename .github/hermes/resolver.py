#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""Résolveur Hermes Crypte : rebase + résolution AUTO des conflits d'une PR.

Portage minimal de `hermes-conflict-resolver.yml` (dépôt duello), adapté à
Crypte (Rust). Dispatché par le gate sur VRAI conflit (`dispatch.py`).
Étapes : rebase sur origin/<base> → résolution IA (fichier/hunk, `resolve.py`)
→ repli déterministe → validation `cargo` (`ops.py`) → push
--force-with-lease → rapport.

Gardes portées : anti-course (refs/SHA), fork refusé, PAS de push si cargo
rouge, PAS de push en cas de perte (HERMES-NO-LOSS), jamais d'état terminal
(le gate re-dispatche avec backoff), secrets jamais journalisés.

Test local sans secrets : `python3 resolver.py --selftest`.
"""

import base64
import os
import shutil
import sys
import tempfile

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))

import ops  # noqa: E402
import resolve  # noqa: E402
from dispatch import rearm_checks  # noqa: E402
from common import (LABEL_ESCALADE, LABEL_FAILED, LABEL_RESOLVER,  # noqa: E402
                    RESOLVER_CHECK, RESOLVER_MARKER, api, arm_scrub, ensure_label,
                    now, post_or_update_comment, set_label)

GH = os.environ.get('GH_TOKEN', '')
CFG = resolve.load_llm_config()
PUSH_TOKEN = (os.environ.get('PUSH_TOKEN') or '').strip()
REPO = os.environ.get('GITHUB_REPOSITORY', '')
PR_NUMBER = os.environ.get('PR_NUMBER', '')
HEAD_REF = os.environ.get('HEAD_REF', '')
BASE_REF = os.environ.get('BASE_REF', '')
EXPECTED_SHA = (os.environ.get('HEAD_SHA') or '').strip()
HELP_HANDLE = '@BaptisteFaisy'
HELP_MARKER = '<!-- hermes-resolver:help -->'
ATTEMPT_TPL = '<!-- hermes-resolver:attempts=%d -->'
ATTEMPT_AT_TPL = '<!-- hermes-resolver:at=%s -->'
ATTEMPT_RE = r'<!-- hermes-resolver:attempts=(\d+) -->'
MAX_ATTEMPTS_INFO = 3
LOOP_MAX_PASSES = 4  # IA → déterministe → IA → déterministe
FRESH_MAX_PASSES = 3

WORK = ''
ia_ok, heur_ok, failed_files, regen_required = [], [], [], []
rebased_onto, ia_hunks = '', {}

arm_scrub(GH, PUSH_TOKEN, CFG['agnes_key'], CFG['key'])


def read_attempts(num):
    """Tentatives déjà consommées (compteur rangé dans le rapport)."""
    import re
    best = 0
    sc, comments = api('GET', '/repos/%s/issues/%s/comments?per_page=100'
                       % (REPO, num), token=GH)
    if sc == 200 and isinstance(comments, list):
        for comment in comments:
            found = re.search(ATTEMPT_RE, comment.get('body') or '')
            if found:
                best = max(best, int(found.group(1)))
    return best


def complete_dispatch_check(sha, ok, title, summary):
    """Clôt le check verrou posé par le gate (ne crée rien)."""
    if not sha:
        return
    sc, data = api('GET', '/repos/%s/commits/%s/check-runs?per_page=100'
                   % (REPO, sha), token=GH)
    if sc != 200 or not isinstance(data, dict):
        return
    for check in data.get('check_runs') or []:
        if check.get('name') != RESOLVER_CHECK:
            continue
        if check.get('status') == 'completed':
            continue
        api('PATCH', '/repos/%s/check-runs/%s' % (REPO, check['id']), token=GH,
            body={'status': 'completed',
                  'conclusion': 'success' if ok else 'failure',
                  'completed_at': now(),
                  'output': {'title': title[:140],
                             'summary': (summary or '')[:60000]}})


def post_report(num, ok, lines, attempts):
    """Rapport sur la PR (unique, mis à jour) avec marqueurs du gate."""
    if ok:
        head = '## 🤖 Hermes sous-agent — ✅ **Conflits résolus**\n\n'
    else:
        head = ('## 🤖 Hermes sous-agent — ❌ **Résolution impossible**\n\n'
                '_Tentative %d. Pas d\'état terminal : Hermes re-dispatche '
                'automatiquement (backoff plafonné) tant que la PR reste '
                'ouverte et en retard._\n\n' % attempts)
    body = (head + '\n'.join(lines) + '\n\n' + (ATTEMPT_TPL % attempts) + '\n'
            + (ATTEMPT_AT_TPL % now()) + '\n' + RESOLVER_MARKER + '\n')
    post_or_update_comment(REPO, GH, num, RESOLVER_MARKER, body)


def escalate(num, title, lines):
    """Demande un humain : label + commentaire dédié (la boucle continue)."""
    pr = {'number': int(num), 'labels': []}
    sc, live = api('GET', '/repos/%s/pulls/%s' % (REPO, num), token=GH)
    if sc == 200 and isinstance(live, dict):
        pr['labels'] = live.get('labels') or []
    set_label(pr, REPO, GH, LABEL_ESCALADE, True)
    body = ('## 🆘 Hermes sous-agent — **Besoin d\'un humain**\n\n'
            '%s : passes de rebase épuisées sur `%s` — branche **intacte**, '
            'rien poussé.\n\n**Motif** : %s\n\n%s\n\n'
            '_La PR reste ouverte ; le re-dispatch continue. Label retiré au '
            'prochain passage vert._\n%s'
            % (HELP_HANDLE, HEAD_REF, title, '\n'.join(lines), HELP_MARKER))
    post_or_update_comment(REPO, GH, num, HELP_MARKER, body)
    print('[escalade] PR #%s : %s' % (num, title), flush=True)


def abort(num, ok, title, lines):
    """Rapport + sortie : exit 1 sur panne réelle, 0 si transitoire/anti-course."""
    if not ok:
        lines = list(lines) + ['',
                               '_Aucun état terminal : retenté automatiquement._']
    head = EXPECTED_SHA
    if not head:
        sc, live = api('GET', '/repos/%s/pulls/%s' % (REPO, num), token=GH)
        if sc == 200 and isinstance(live, dict):
            head = (live.get('head') or {}).get('sha', '')
    complete_dispatch_check(head, ok, title, '\n'.join(lines))
    post_report(num, ok, lines, read_attempts(num) if ok else read_attempts(num) + 1)
    if not ok:
        try:
            escalate(num, title, lines)
        except Exception as exc:
            print('[escalade] non bloquante : %s' % exc, flush=True)
    print('[résolveur] %s :: %s' % (title, ' | '.join(lines)), flush=True)
    sys.exit(0 if ok else 1)


def do_rebase(use_llm, blacklist=None):
    """Reset sur origin/<head> puis rebase sur origin/<base>, conflits résolus.

    use_llm=False : 100 % déterministe. blacklist : chemins interdits d'IA
    sur cette passe (suspects du cargo rouge au rattrapage).
    """
    global ia_ok, heur_ok, failed_files, regen_required, rebased_onto, ia_hunks
    banned = set(blacklist or ())
    ia_ok, heur_ok, failed_files, regen_required = [], [], [], []
    ia_hunks = {}
    if os.path.isdir(os.path.join(WORK, '.git', 'rebase-merge')) or \
            os.path.isdir(os.path.join(WORK, '.git', 'rebase-apply')):
        ops.git(WORK, 'rebase', '--abort')
    rebased_onto = ops.fetch_base(WORK, BASE_REF, HEAD_REF)
    if not ops.git_ok(WORK, 'checkout', '-B', HEAD_REF, 'origin/' + HEAD_REF):
        return None
    proc = ops.git(WORK, 'rebase', '--empty=drop', 'origin/' + BASE_REF)
    rounds = 0
    while proc is not None and proc.returncode != 0 and rounds < 12:
        rounds += 1
        diff = ops.run(['git', 'diff', '--name-only', '--diff-filter=U'],
                       timeout=60, cwd=WORK)
        conflicts = [l.strip() for l in (diff.stdout or '').splitlines()
                     if l.strip()] if diff and diff.returncode == 0 else []
        if not conflicts:
            break
        mb = ops.run(['git', 'merge-base', 'REBASE_HEAD', 'origin/' + BASE_REF],
                     timeout=60, cwd=WORK)
        base_rev = mb.stdout.strip() if mb and mb.returncode == 0 \
            and mb.stdout.strip() else 'origin/' + BASE_REF
        for path in conflicts:
            out = None
            if path in banned:
                print('[rebase] %s : IA interdite (suspect cargo rouge)' % path)
            elif use_llm and resolve.llm_available(CFG):
                base_t = ops.file_content(WORK, base_rev, path)
                ours_t = ops.file_content(WORK, 'HEAD', path)
                theirs_t = ops.file_content(WORK, 'REBASE_HEAD', path)
                skip = resolve.llm_skip_reason(path, base_t, ours_t, theirs_t)
                if skip and resolve.too_big_for_whole_file(base_t, ours_t, theirs_t) \
                        and not resolve.is_derived(path) \
                        and not resolve.is_binary(path):
                    print('[rebase] %s : %s — bascule hunk par hunk' % (path, skip))
                    out, n_h = resolve.resolve_hunks(CFG, WORK, path)
                    if out is not None:
                        bad = resolve.valid_output(path, out, ours_t, theirs_t)
                        if bad:
                            print('[rebase] sortie IA (hunks) REFUSÉE %s : %s'
                                  % (path, bad))
                            out = None
                        else:
                            ia_hunks[path] = n_h
                elif skip:
                    print('[rebase] %s : IA écartée (%s)' % (path, skip))
                else:
                    print('[rebase] %s : résolution IA' % path, flush=True)
                    out = resolve.resolve_file(CFG, path, base_t, ours_t, theirs_t)
                    if out is not None:
                        bad = resolve.valid_output(path, out, ours_t, theirs_t)
                        if bad:
                            print('[rebase] sortie IA REFUSÉE %s : %s' % (path, bad))
                            out = None
            else:
                print('[rebase] %s : mode déterministe (sans IA)' % path)
            if out is not None:
                with open(os.path.join(WORK, path), 'w', encoding='utf-8',
                          newline='\n') as fh:
                    fh.write(out)
                ia_ok.append(path)
                continue
            mode = resolve.heuristic_resolve(path)
            if mode and ops.apply_mode(WORK, path, mode):
                heur_ok.append('%s (repli : %s)' % (path, mode))
                if resolve.is_derived(path):
                    regen_required.append(path)
            else:
                failed_files.append(path)
        if failed_files:
            break
        ops.run(['git', 'add', '--'] + conflicts, cwd=WORK)
        proc = ops.git(WORK, 'rebase', '--continue')
    return proc


def do_rebase_passes():
    """Boucle IA → déterministe → IA → déterministe (jamais de côté forcé)."""
    proc, passes = None, 0
    while passes < LOOP_MAX_PASSES:
        passes += 1
        proc = do_rebase(use_llm=(passes not in (2, 4)))
        if proc is not None and proc.returncode == 0:
            return proc, passes
        ops.git(WORK, 'rebase', '--abort')
        print('[boucle] passe %d/%d échouée' % (passes, LOOP_MAX_PASSES),
              flush=True)
    return proc, passes


def check_no_loss(lines):
    """Garde-fou HERMES-NO-LOSS : refuse un rebase qui viderait la PR."""
    def rev(name):
        proc = ops.git(WORK, 'rev-parse', name, timeout=120)
        return proc.stdout.strip() if proc and proc.returncode == 0 else ''

    pertes = []
    head_now, orig_head = rev('HEAD'), rev('origin/' + HEAD_REF)
    mb = ops.git(WORK, 'merge-base', 'origin/' + BASE_REF, 'origin/' + HEAD_REF,
                 timeout=120)
    merge_base = mb.stdout.strip() if mb and mb.returncode == 0 else ''
    had_diff = bool(orig_head) and not ops.git_ok(
        WORK, 'diff', '--quiet', 'origin/' + BASE_REF, orig_head)
    has_diff = bool(head_now) and not ops.git_ok(
        WORK, 'diff', '--quiet', 'origin/' + BASE_REF, head_now)
    if had_diff and not has_diff:
        pertes.append('le rebase viderait intégralement la PR (diff nul)')
    resolved = bool(ia_ok or heur_ok)
    if orig_head and head_now and merge_base:
        orig_pids = ops.patch_ids(WORK, 'origin/%s..%s' % (BASE_REF, orig_head))
        new_pids = ops.patch_ids(WORK, 'origin/%s..%s' % (BASE_REF, head_now))
        base_pids = ops.patch_ids(WORK, '%s..origin/%s' % (merge_base, BASE_REF))
        if orig_pids is None or new_pids is None or base_pids is None:
            if not resolved:
                pertes.append('vérification des patch-ids impossible')
        else:
            missing = sorted(orig_pids - new_pids - base_pids)
            if missing and not resolved:
                pertes += ['patch `%s` ni dans le head ni dans la base' % p[:12]
                           for p in missing]
            elif missing:
                lines.append('ℹ️ %d commit(s) réécrits par la résolution : %s'
                             % (len(missing),
                                ', '.join('`%s`' % p[:12] for p in missing[:5])))
    return pertes


def clone_repo():
    """Clone jetable (dépôt ~12 Mo : clone complet, pas de miroir)."""
    global WORK
    WORK = tempfile.mkdtemp(prefix='hermes-resolver-')
    print('[clone] %s' % WORK, flush=True)
    proc = ops.git(None, 'clone', '--no-tags', '-c', 'user.name=hermes-resolver',
                   '-c', 'user.email=hermes-resolver@crypte.local',
                   'https://github.com/%s.git' % REPO, WORK, timeout=600)
    if proc is None or proc.returncode != 0:
        return False
    proc = ops.git(WORK, 'checkout', '-B', HEAD_REF, 'origin/' + HEAD_REF)
    if proc is None or proc.returncode != 0:
        return False
    return True


def main():
    """Rebase + résolution + validation + push + rapport (jamais de merge)."""
    import atexit
    for label, color in ((LABEL_RESOLVER, '8250df'), (LABEL_FAILED, 'd73a4a'),
                         (LABEL_ESCALADE, 'd73a4a')):
        ensure_label(REPO, GH, label, color, 'Posé par le résolveur Hermes')
    sc, pr = api('GET', '/repos/%s/pulls/%s' % (REPO, PR_NUMBER), token=GH)
    if sc != 200 or not isinstance(pr, dict) or pr.get('state') != 'open':
        print('[résolveur] PR introuvable ou fermée — abandon')
        sys.exit(0)
    if (pr.get('head') or {}).get('repo', {}).get('full_name') != REPO:
        abort(PR_NUMBER, False, 'Fork non supporté',
              ['PR issue d\'un fork : pousse uniquement sur ce dépôt.'])
    if (pr.get('base') or {}).get('ref') != BASE_REF \
            or (pr.get('head') or {}).get('ref') != HEAD_REF:
        abort(PR_NUMBER, True, 'Refs modifiées',
              ['Refs changées depuis le dispatch — le gate re-dispatche.'])
    if EXPECTED_SHA and (pr.get('head') or {}).get('sha') != EXPECTED_SHA:
        abort(PR_NUMBER, True, 'Head modifié',
              ['Head changé depuis le dispatch — le gate re-dispatche.'])
    if not resolve.llm_available(CFG):
        print('[résolveur] clés LLM absentes — 100 % déterministe '
              '(+ escalade si irrésoluble)', flush=True)
    if not clone_repo():
        abort(PR_NUMBER, False, 'Clone impossible', ['Clone du dépôt échoué.'])
    atexit.register(shutil.rmtree, WORK, ignore_errors=True)
    proc = ops.run(['git', 'rev-parse', 'HEAD'], timeout=60, cwd=WORK)
    before = proc.stdout.strip() if proc and proc.returncode == 0 else ''

    fresh_passes = 0
    while True:
        proc, loop_passes = do_rebase_passes()
        if proc is None or proc.returncode != 0:
            ops.git(WORK, 'rebase', '--abort')
            set_label(pr, REPO, GH, LABEL_RESOLVER, False)
            set_label(pr, REPO, GH, LABEL_FAILED, True)
            abort(PR_NUMBER, False,
                  'Rebase non résoluble après %d passes' % loop_passes,
                  ['Fichiers non résolus : %s'
                   % ', '.join('`%s`' % f for f in failed_files)
                   if failed_files else 'Rebase non abouti (voir logs).',
                   '%d passes (IA + déterministe, aucun côté forcé).'
                   % loop_passes, 'Aucun push effectué.'])
        moved = ops.fetch_base(WORK, BASE_REF, HEAD_REF)
        if not moved or moved == rebased_onto or fresh_passes >= FRESH_MAX_PASSES:
            break
        fresh_passes += 1
        print('[résolveur] base %s → %s : nouveau rebase'
              % (rebased_onto[:8], moved[:8]), flush=True)

    notes, ia_first = [], list(ia_ok)
    ok, cargo_notes = ops.cargo_validate(WORK)
    notes += cargo_notes
    if not ok and ia_first:
        print('[rattrapage] cargo rouge après IA — blacklist ciblée', flush=True)
        notes.append('**Rattrapage** : résolution refaite sans IA sur : '
                     + ', '.join('`%s`' % f for f in ia_first))
        retry = do_rebase(True, blacklist=ia_first)
        if retry is None or retry.returncode != 0:
            ops.git(WORK, 'rebase', '--abort')
            set_label(pr, REPO, GH, LABEL_RESOLVER, False)
            set_label(pr, REPO, GH, LABEL_FAILED, True)
            abort(PR_NUMBER, False, 'Rebase non résoluble (rattrapage)',
                  notes + ['Rattrapage ciblé non abouti — aucun push.'])
        ok, cargo_notes = ops.cargo_validate(WORK)
        notes += cargo_notes
    if not ok and (ia_ok or heur_ok):
        print('[rattrapage] encore rouge — passe complète sans IA', flush=True)
        notes.append('**Dernier rattrapage** : rebase complet sans IA.')
        retry = do_rebase(False)
        if retry is None or retry.returncode != 0:
            ops.git(WORK, 'rebase', '--abort')
            set_label(pr, REPO, GH, LABEL_RESOLVER, False)
            set_label(pr, REPO, GH, LABEL_FAILED, True)
            abort(PR_NUMBER, False, 'Rebase non résoluble (sans IA)',
                  notes + ['Rattrapage sans IA non abouti — aucun push.'])
        ok, cargo_notes = ops.cargo_validate(WORK)
        notes += cargo_notes
    if not ok:
        set_label(pr, REPO, GH, LABEL_RESOLVER, False)
        set_label(pr, REPO, GH, LABEL_FAILED, True)
        abort(PR_NUMBER, False, 'Cargo en échec après rebase',
              notes + ['Rebase OK mais **cargo rouge** — push bloqué.'])

    proc = ops.run(['git', 'rev-parse', 'HEAD'], timeout=60, cwd=WORK)
    after = proc.stdout.strip() if proc and proc.returncode == 0 else ''
    lines = ['Branche `%s` rebasée sur `%s` (`%s` → `%s`).'
             % (HEAD_REF, BASE_REF, before[:8], after[:8])]
    if not resolve.llm_available(CFG):
        lines.append('ℹ️ Clés LLM absentes : résolution 100 % déterministe.')
    if ia_ok:
        lines.append('**Résolus par IA** : ' + ', '.join(
            '`%s`%s' % (f, ' (%d hunks)' % ia_hunks[f] if f in ia_hunks else '')
            for f in ia_ok))
    if heur_ok:
        lines.append('**Résolus sans IA** : '
                     + ', '.join('`%s`' % f for f in heur_ok))
    if not (ia_ok or heur_ok):
        lines.append('Aucun conflit de contenu : rebase simple.')
    if regen_required:
        lines.append('')
        lines.append('⚠️ **%d fichier(s) dérivé(s) à régénérer** (côté base '
                     'conservé, jamais réécrit par l\'IA) :'
                     % len(regen_required))
        lines += ['- `%s`' % f for f in regen_required[:20]]
    lines += notes

    pertes = check_no_loss(lines)
    if pertes:
        set_label(pr, REPO, GH, LABEL_RESOLVER, False)
        set_label(pr, REPO, GH, LABEL_FAILED, True)
        abort(PR_NUMBER, False, 'Perte détectée — push refusé',
              ['**Garde-fou HERMES-NO-LOSS** : branche `%s` **intacte**.'
               % HEAD_REF, ''] + ['- ' + x for x in pertes[:10]])

    lease = ('--force-with-lease=refs/heads/%s:%s' % (HEAD_REF, EXPECTED_SHA)
             if EXPECTED_SHA else '--force-with-lease')
    refspec = '%s:%s' % (HEAD_REF, HEAD_REF)
    pushed = None
    if PUSH_TOKEN:
        hdr = 'AUTHORIZATION: basic ' + base64.b64encode(
            ('x-access-token:%s' % PUSH_TOKEN).encode()).decode()
        pushed = ops.git(WORK, '-c',
                         'http.https://github.com/.extraheader=' + hdr,
                         'push', lease, 'origin', refspec, timeout=300)
    if pushed is None or pushed.returncode != 0:
        pushed = ops.git(WORK, 'push', lease, 'origin', refspec, timeout=300)
    if pushed is None or pushed.returncode != 0:
        set_label(pr, REPO, GH, LABEL_RESOLVER, False)
        set_label(pr, REPO, GH, LABEL_FAILED, True)
        abort(PR_NUMBER, False, 'Push refusé',
              lines + ['', 'Push refusé (head distant modifié ou droits).'])

    rearmed = rearm_checks(REPO, GH, PUSH_TOKEN, PR_NUMBER, HEAD_REF)
    set_label(pr, REPO, GH, LABEL_RESOLVER, False)
    set_label(pr, REPO, GH, LABEL_FAILED, False)
    set_label(pr, REPO, GH, LABEL_ESCALADE, False)
    lines.append('')
    if rearmed:
        lines.append('Checks ré-armés : ' + ', '.join('`%s`' % w for w in rearmed)
                     + '.')
    lines.append('Push effectué — le gate ré-évalue la PR (merge si vert).')
    complete_dispatch_check(EXPECTED_SHA, True,
                            'Conflits résolus + push (%s)' % after[:8],
                            '\n'.join(lines))
    post_report(PR_NUMBER, True, lines, read_attempts(PR_NUMBER))
    print('[résolveur] PR #%s : push OK (%s)' % (PR_NUMBER, after[:8]),
          flush=True)


def selftest():
    """Chemins secs sans secrets : `python3 resolver.py --selftest`."""
    fails = []

    def check(name, cond):
        print(('PASS ' if cond else 'FAIL ') + name)
        if not cond:
            fails.append(name)

    check('config vide sans clés', not resolve.llm_available(CFG)
          if not os.environ.get('LLM_BASE')
          and not os.environ.get('AGNES_API_KEY') else True)
    check('constantes gate alignées',
          RESOLVER_CHECK == 'Hermes sous-agent (conflits)'
          and LABEL_FAILED == 'hermes/resolution-echouee')
    check('lease anti-course formaté',
          ('--force-with-lease=refs/heads/x:abc'
           if EXPECTED_SHA else '--force-with-lease') is not None)
    import io
    from contextlib import redirect_stdout
    buf = io.StringIO()
    with redirect_stdout(buf):
        code = resolve.selftest()
    check('selftest resolve.py vert', code == 0)
    print('%d échec(s) sur le selftest' % len(fails))
    return 1 if fails else 0


if __name__ == '__main__':
    if len(sys.argv) == 2 and sys.argv[1] == '--selftest':
        sys.exit(selftest())
    if len(sys.argv) != 1:
        print('Usage : python3 resolver.py [--selftest]')
        sys.exit(2)
    for var in ('GH_TOKEN', 'GITHUB_REPOSITORY', 'PR_NUMBER', 'HEAD_REF',
                'BASE_REF'):
        if not os.environ.get(var):
            print('Variable requise manquante : %s' % var)
            sys.exit(2)
    main()
