// Original AS3 RegExp facade. Primary contracts: AIR RegExp/String references;
// constructor/clone behavior cross-checked against Adobe avmplus RegExpClass.cpp
// at 858d034a3bd3a54d9b70909386435cf4aec81d21. No upstream code is copied.
// https://airsdk.dev/reference/actionscript/3.0/RegExp.html
// https://airsdk.dev/reference/actionscript/3.0/String.html
// Supports g/i/m/s/x flags, PCRE named groups (?P<name>...)/(?<name>...)/(?P=name)/\k<name>,
// inline flags (?imsx-imsx) and (?imsx-imsx:...), PCRE conditionals (?(1)yes|no),
// (?(name)yes|no) and (?(?=...)yes|no), inline (?#...) comments, and \a/\e/\A/\Z/\z/\x{...}.
const __nfRegExpNative = RegExp;
const __nfRegExpNativeExec = RegExp.prototype.exec;
const __nfRegExpApply = Reflect.apply;
const __nfRegExpBrand = Symbol.for('neoflash.as3.regexp.v1');
const __nfRegExpDescriptor = Object.getOwnPropertyDescriptor;
const __nfRegExpLimit = 1000000;
let __nfRegExpFacadeClass;
function __nfRegExpUnsupported(detail) { throw new __nfTypeError('Unsupported AS3 RegExp: ' + detail); }
function __nfRegExpSyntaxError(detail) { throw new __nfErrorTypes.SyntaxError('Invalid or unsupported AS3 RegExp pattern: ' + detail); }
function __nfRegExpIs(value) {
    if (value === null || (typeof value !== 'object' && typeof value !== 'function')) return false;
    const descriptor = __nfRegExpDescriptor(value, __nfRegExpBrand);
    return !!descriptor && Object.hasOwn(descriptor, 'value') && descriptor.value != null && descriptor.value.version === 1;
}
function __nfRegExpState(value) {
    if (!__nfRegExpIs(value)) throw new __nfTypeError('Expected an AS3 RegExp.');
    return __nfRegExpDescriptor(value, __nfRegExpBrand).value;
}
function __nfRegExpText(value, name, limit = __nfRegExpLimit) {
    const text = __nfString(value);
    if (text.length > limit) __nfRegExpUnsupported(name + ' exceeds ' + limit + ' UTF-16 code units.');
    return text;
}
function __nfEncodeCodePointEscape(cp, inClass) {
    if (!Number.isInteger(cp) || cp < 0 || cp > 0x10ffff) __nfRegExpSyntaxError('\\x{...} code point out of Unicode range.');
    if (cp <= 0xffff) return '\\u' + cp.toString(16).padStart(4, '0');
    if (inClass) __nfRegExpUnsupported('astral \\x{...} inside character class requires u-mode.');
    const v = cp - 0x10000;
    const hi = 0xd800 + (v >> 10), lo = 0xdc00 + (v & 0x3ff);
    return '(?:\\u' + hi.toString(16).padStart(4, '0') + '\\u' + lo.toString(16).padStart(4, '0') + ')';
}
function __nfApplyInlineFlags(baseFlags, spec) {
    const next = { ...baseFlags };
    let enable = true;
    for (let i = 0; i < spec.length; i++) {
        const ch = spec[i];
        if (ch === '-') { enable = false; continue; }
        if (!'imsx'.includes(ch)) __nfRegExpSyntaxError('unsupported inline flag ' + ch);
        next[ch] = enable;
    }
    return next;
}
function __nfWrapScopedJsFlags(inner, prevFlags, curFlags) {
    let on = '', off = '';
    for (const f of ['i', 'm', 's']) {
        if (curFlags[f] !== prevFlags[f]) {
            if (curFlags[f]) on += f; else off += f;
        }
    }
    if (!on && !off) return inner;
    return '(?' + on + (off ? '-' + off : '') + ':' + inner + ')';
}
function __nfCompilePcre(source, initialFlags) {
    const conditionals = [];
    const groupNames = new Map();
    let captureCount = 0;
    let pos = 0;

    function findBalancedParen(start) {
        let depth = 1, inCls = false;
        for (let i = start; i < source.length; i++) {
            const ch = source[i];
            if (ch === '\\') { i++; continue; }
            if (ch === '[' && !inCls) { inCls = true; continue; }
            if (ch === ']' && inCls) { inCls = false; continue; }
            if (!inCls) {
                if (ch === '(') depth++;
                else if (ch === ')') { depth--; if (depth === 0) return i; }
            }
        }
        __nfRegExpSyntaxError('unterminated group');
    }

    function splitTopAlternations(seg) {
        const parts = [];
        let cur = '', depth = 0, inCls = false;
        for (let i = 0; i < seg.length; i++) {
            const ch = seg[i];
            if (ch === '\\') { cur += ch + (seg[++i] ?? ''); continue; }
            if (ch === '[' && !inCls) { inCls = true; cur += ch; continue; }
            if (ch === ']' && inCls) { inCls = false; cur += ch; continue; }
            if (!inCls) {
                if (ch === '(') depth++;
                else if (ch === ')') depth--;
                else if (ch === '|' && depth === 0) { parts.push(cur); cur = ''; continue; }
            }
            cur += ch;
        }
        parts.push(cur);
        return parts;
    }

    function parseSequence(stopChar, activeFlags) {
        let currentFlags = { ...activeFlags };
        const segments = [];
        let buf = '';
        let inClass = false;

        function flushSegment() {
            if (buf.length > 0 || segments.length > 0) {
                segments.push({ text: buf, flags: { ...currentFlags } });
                buf = '';
            }
        }

        while (pos < source.length) {
            const ch = source[pos];
            if (!inClass && stopChar && ch === stopChar) break;
            if (!inClass && ch === '|') {
                flushSegment();
                const wrappedBranch = segments.reduceRight((acc, seg, idx) => {
                    const base = idx === 0 ? activeFlags : segments[idx - 1].flags;
                    return __nfWrapScopedJsFlags(seg.text + acc, base, seg.flags);
                }, '');
                pos++;
                const rest = parseSequence(stopChar, activeFlags);
                return wrappedBranch + '|' + rest;
            }
            if (ch === '\\') {
                const next = source[++pos];
                if (next === undefined) { buf += '\\'; pos++; break; }
                pos++;
                if (next === 'a') { buf += '\\x07'; continue; }
                if (next === 'e') { buf += '\\x1b'; continue; }
                if (!inClass && next === 'A') { buf += '(?<![\\s\\S])'; continue; }
                if (!inClass && next === 'z') { buf += '(?![\\s\\S])'; continue; }
                if (!inClass && next === 'Z') { buf += '(?=\\n?(?![\\s\\S]))'; continue; }
                if (next === 'x' && source[pos] === '{') {
                    const close = source.indexOf('}', pos + 1);
                    if (close < 0) __nfRegExpSyntaxError('unterminated \\x{...} escape.');
                    const hex = source.slice(pos + 1, close);
                    if (!/^[0-9a-fA-F]{1,6}$/.test(hex)) __nfRegExpSyntaxError('invalid \\x{...} hex digits.');
                    pos = close + 1;
                    buf += __nfEncodeCodePointEscape(parseInt(hex, 16), inClass);
                    continue;
                }
                if (next === 'k' && source[pos] === '<') {
                    const close = source.indexOf('>', pos + 1);
                    if (close < 0) __nfRegExpSyntaxError('unterminated \\k<name> backreference.');
                    const name = source.slice(pos + 1, close);
                    pos = close + 1;
                    buf += '\\k<' + name + '>';
                    continue;
                }
                if (/[gGhHKNoOpPQRX]/.test(next) || (next === 'u' && source[pos] === '{')) {
                    __nfRegExpUnsupported('PCRE-only escape \\' + next + ' is not available in the JavaScript pattern backend.');
                }
                buf += '\\' + next;
                continue;
            }
            if (!inClass && currentFlags.x && /[\t\n\r\f\v ]/.test(ch)) { pos++; continue; }
            if (!inClass && currentFlags.x && ch === '#') {
                pos++;
                while (pos < source.length && !/[\n\r]/.test(source[pos])) pos++;
                continue;
            }
            if (!inClass && ((ch === '*' || ch === '+' || ch === '?' || ch === '}') && source[pos + 1] === '+')) {
                __nfRegExpUnsupported('possessive quantifiers are not supported by the JavaScript backend.');
            }
            if (ch === '[' && !inClass) { inClass = true; buf += ch; pos++; continue; }
            if (ch === ']' && inClass) { inClass = false; buf += ch; pos++; continue; }
            if (!inClass && ch === '(') {
                pos++;
                if (source[pos] !== '?') {
                    captureCount++;
                    const inner = parseSequence(')', currentFlags);
                    if (source[pos] !== ')') __nfRegExpSyntaxError('unterminated capturing group.');
                    pos++;
                    buf += '(' + inner + ')';
                    continue;
                }
                pos++; // skip '?'
                if (source[pos] === '#') {
                    const endComment = source.indexOf(')', pos + 1);
                    if (endComment < 0) __nfRegExpSyntaxError('unterminated (?#...) comment.');
                    pos = endComment + 1;
                    continue;
                }
                if (source[pos] === ':' || source[pos] === '=' || source[pos] === '!') {
                    const kind = source[pos++];
                    const inner = parseSequence(')', currentFlags);
                    if (source[pos] !== ')') __nfRegExpSyntaxError('unterminated group.');
                    pos++;
                    buf += '(?' + kind + inner + ')';
                    continue;
                }
                if (source[pos] === '<' && (source[pos + 1] === '=' || source[pos + 1] === '!')) {
                    const kind = source.slice(pos, pos + 2);
                    pos += 2;
                    const inner = parseSequence(')', currentFlags);
                    if (source[pos] !== ')') __nfRegExpSyntaxError('unterminated lookbehind.');
                    pos++;
                    buf += '(?' + kind + inner + ')';
                    continue;
                }
                if (source[pos] === '<' || (source[pos] === 'P' && source[pos + 1] === '<')) {
                    if (source[pos] === 'P') pos++;
                    pos++; // skip '<'
                    const close = source.indexOf('>', pos);
                    if (close < 0) __nfRegExpSyntaxError('unterminated named group.');
                    const name = source.slice(pos, close);
                    if (!/^[A-Za-z_$][A-Za-z0-9_$]*$/.test(name)) __nfRegExpSyntaxError('invalid group name ' + name);
                    pos = close + 1;
                    const idx = ++captureCount;
                    groupNames.set(name, idx);
                    const inner = parseSequence(')', currentFlags);
                    if (source[pos] !== ')') __nfRegExpSyntaxError('unterminated named group.');
                    pos++;
                    buf += '(?<' + name + '>' + inner + ')';
                    continue;
                }
                if (source[pos] === 'P' && source[pos + 1] === '=') {
                    pos += 2;
                    const close = source.indexOf(')', pos);
                    if (close < 0) __nfRegExpSyntaxError('unterminated (?P=name) backreference.');
                    const name = source.slice(pos, close);
                    pos = close + 1;
                    buf += '\\k<' + name + '>';
                    continue;
                }
                if (source[pos] === '(') {
                    // Conditional group: (?(cond)yes|no) or (?(cond)yes)
                    const condStart = pos + 1;
                    let condEnd;
                    if (source[condStart] === '?' && ['=', '!', '<'].includes(source[condStart + 1])) {
                        condEnd = findBalancedParen(condStart);
                        const lookRaw = source.slice(pos, condEnd + 1);
                        pos = condEnd + 1;
                        const bodyEnd = findBalancedParen(pos);
                        const bodyRaw = source.slice(pos, bodyEnd);
                        pos = bodyEnd + 1;
                        const branches = splitTopAlternations(bodyRaw);
                        if (branches.length > 2) __nfRegExpSyntaxError('conditional group accepts at most two branches.');
                        const subYes = __nfCompileSubpattern(branches[0], currentFlags);
                        const subNo = branches.length === 2 ? __nfCompileSubpattern(branches[1], currentFlags) : '';
                        const subLook = __nfCompileSubpattern(lookRaw, currentFlags);
                        const strippedLookBody = lookRaw.startsWith('(?<=') || lookRaw.startsWith('(?<!')
                            ? __nfStripCaptures(subLook.slice(4))
                            : __nfStripCaptures(subLook.slice(3));
                        const negLook = lookRaw.startsWith('(?=')
                            ? '(?!' + strippedLookBody
                            : lookRaw.startsWith('(?!')
                                ? '(?=' + strippedLookBody
                                : lookRaw.startsWith('(?<=')
                                    ? '(?<!' + strippedLookBody
                                    : '(?<=' + strippedLookBody;
                        buf += '(?:' + subLook + subYes + '|' + negLook + subNo + ')';
                        continue;
                    }
                    condEnd = source.indexOf(')', condStart);
                    if (condEnd < 0) __nfRegExpSyntaxError('unterminated conditional reference.');
                    const refSpec = source.slice(condStart, condEnd).trim();
                    pos = condEnd + 1;
                    const bodyEnd = findBalancedParen(pos);
                    const bodyRaw = source.slice(pos, bodyEnd);
                    pos = bodyEnd + 1;
                    const branches = splitTopAlternations(bodyRaw);
                    if (branches.length > 2) __nfRegExpSyntaxError('conditional group accepts at most two branches.');
                    if (conditionals.length >= 8) __nfRegExpUnsupported('more than 8 group conditionals in one pattern.');
                    const condId = conditionals.length;
                    conditionals.push({ ref: refSpec, yes: '', no: '' });
                    const savedPos = pos, savedSrc = source;
                    source = branches[0]; pos = 0;
                    const yesCompiled = parseSequence(null, currentFlags);
                    let noCompiled = '';
                    if (branches.length === 2) {
                        source = branches[1]; pos = 0;
                        noCompiled = parseSequence(null, currentFlags);
                    }
                    source = savedSrc; pos = savedPos;
                    conditionals[condId].yes = yesCompiled;
                    conditionals[condId].no = noCompiled;
                    buf += '\0COND' + condId + '\0';
                    continue;
                }
                const flagMatch = /^([imsx]*(?:-[imsx]+)?)(:|\))/.exec(source.slice(pos));
                if (flagMatch) {
                    const spec = flagMatch[1], delim = flagMatch[2];
                    if (!spec) __nfRegExpSyntaxError('empty inline modifier group.');
                    pos += flagMatch[0].length;
                    const nextFlags = __nfApplyInlineFlags(currentFlags, spec);
                    if (delim === ':') {
                        const inner = parseSequence(')', nextFlags);
                        if (source[pos] !== ')') __nfRegExpSyntaxError('unterminated inline flag group.');
                        pos++;
                        buf += __nfWrapScopedJsFlags(inner, currentFlags, nextFlags);
                    } else {
                        flushSegment();
                        currentFlags = nextFlags;
                    }
                    continue;
                }
                __nfRegExpUnsupported('unsupported (?...) group extension in AS3 RegExp.');
            }
            buf += ch;
            pos++;
        }

        flushSegment();
        if (segments.length === 0) return '';
        return segments.reduceRight((acc, seg, idx) => {
            const base = idx === 0 ? activeFlags : segments[idx - 1].flags;
            return __nfWrapScopedJsFlags(seg.text + acc, base, seg.flags);
        }, '');
    }

    function __nfCompileSubpattern(sub, flagsObj) {
        const savedSrc = source, savedPos = pos;
        source = sub; pos = 0;
        try { return parseSequence(null, flagsObj); }
        finally { source = savedSrc; pos = savedPos; }
    }

    const compiledTemplate = parseSequence(null, initialFlags);
    if (pos < source.length) __nfRegExpSyntaxError('unexpected closing parenthesis.');
    return { compiledTemplate, conditionals, groupNames };
}
function __nfStripCaptures(frag) {
    let out = '', inClass = false;
    for (let i = 0; i < frag.length; i++) {
        const ch = frag[i];
        if (ch === '\\') {
            out += ch + (frag[i + 1] ?? '');
            i++;
            continue;
        }
        if (ch === '[') { inClass = true; out += ch; continue; }
        if (ch === ']' && inClass) { inClass = false; out += ch; continue; }
        if (!inClass && ch === '(') {
            if (frag[i + 1] !== '?') {
                out += '(?:';
                continue;
            }
            if (frag[i + 2] === '<' && frag[i + 3] !== '=' && frag[i + 3] !== '!') {
                const closeAngle = frag.indexOf('>', i + 3);
                if (closeAngle > i + 2) {
                    out += '(?:';
                    i = closeAngle;
                    continue;
                }
            }
        }
        out += ch;
    }
    return out;
}
function __nfCollectNamedCaptures(frag) {
    const names = new Set();
    let inClass = false;
    for (let i = 0; i < frag.length; i++) {
        const ch = frag[i];
        if (ch === '\\') { i++; continue; }
        if (ch === '[') { inClass = true; continue; }
        if (ch === ']' && inClass) { inClass = false; continue; }
        if (!inClass && ch === '(' && frag[i + 1] === '?' && frag[i + 2] === '<' && frag[i + 3] !== '=' && frag[i + 3] !== '!') {
            const closeAngle = frag.indexOf('>', i + 3);
            if (closeAngle > i + 2) {
                names.add(frag.slice(i + 3, closeAngle));
                i = closeAngle;
            }
        }
    }
    return names;
}
function __nfDummyCaptures(frag, excludeNames) {
    let out = '', inClass = false;
    for (let i = 0; i < frag.length; i++) {
        const ch = frag[i];
        if (ch === '\\') { i++; continue; }
        if (ch === '[') { inClass = true; continue; }
        if (ch === ']' && inClass) { inClass = false; continue; }
        if (!inClass && ch === '(') {
            if (frag[i + 1] !== '?') {
                out += '()';
                continue;
            }
            if (frag[i + 2] === '<' && frag[i + 3] !== '=' && frag[i + 3] !== '!') {
                const closeAngle = frag.indexOf('>', i + 3);
                if (closeAngle > i + 2) {
                    const name = frag.slice(i + 3, closeAngle);
                    out += excludeNames && excludeNames.has(name) ? '()' : '(?<' + name + '>)';
                    i = closeAngle;
                }
            }
        }
    }
    return out;
}
function __nfAttachNamedGroups(match) {
    if (match && match.groups) {
        for (const [k, v] of Object.entries(match.groups)) {
            if (!(k in match)) match[k] = v;
        }
    }
    return match;
}
function __nfBuildVariants(compiledInfo, nativeFlags) {
    const { compiledTemplate, conditionals, groupNames } = compiledInfo;
    if (conditionals.length === 0) {
        const engine = new __nfRegExpNative(compiledTemplate, nativeFlags);
        return [{ engine, sticky: null, checks: [] }];
    }
    const stickyFlags = nativeFlags.replace(/g|y/g, '') + 'y';
    const count = 1 << conditionals.length;
    const variants = [];
    for (let mask = count - 1; mask >= 0; mask--) {
        const expanded = conditionals.map(c => ({ yes: c.yes, no: c.no, ref: c.ref }));
        const replacements = new Array(conditionals.length);
        for (let i = conditionals.length - 1; i >= 0; i--) {
            let yesBranch = expanded[i].yes;
            let noBranch = expanded[i].no;
            for (let j = i + 1; j < conditionals.length; j++) {
                const token = '\0COND' + j + '\0';
                if (yesBranch.includes(token)) yesBranch = yesBranch.replace(token, () => replacements[j]);
                if (noBranch.includes(token)) noBranch = noBranch.replace(token, () => replacements[j]);
            }
            const takeYes = ((mask >> (conditionals.length - 1 - i)) & 1) === 1;
            if (takeYes) {
                const noDummy = __nfDummyCaptures(noBranch, __nfCollectNamedCaptures(yesBranch));
                replacements[i] = noDummy ? '(?:' + yesBranch + '|(?!)(' + noDummy.slice(1) + ')' : '(?:' + yesBranch + ')';
                if (noDummy) replacements[i] = '(?:' + yesBranch + '|(?!)' + noDummy + ')';
            } else {
                const yesDummy = __nfDummyCaptures(yesBranch, __nfCollectNamedCaptures(noBranch));
                replacements[i] = yesDummy ? '(?:(?!)' + yesDummy + '|' + noBranch + ')' : '(?:' + noBranch + ')';
            }
        }
        let pattern = compiledTemplate;
        const checks = [];
        for (let i = 0; i < conditionals.length; i++) {
            const takeYes = ((mask >> (conditionals.length - 1 - i)) & 1) === 1;
            const cond = conditionals[i];
            if (pattern.includes('\0COND' + i + '\0')) {
                pattern = pattern.replace('\0COND' + i + '\0', () => replacements[i]);
            }
            const isNum = /^\d+$/.test(cond.ref);
            const groupIdx = isNum ? Number(cond.ref) : groupNames.get(cond.ref);
            checks.push({ ref: cond.ref, groupIdx, takeYes });
        }
        const engine = new __nfRegExpNative(pattern, nativeFlags);
        const sticky = new __nfRegExpNative(pattern, stickyFlags);
        variants.push({ engine, sticky, checks });
    }
    return variants;
}
function __nfExecVariants(variants, text, startIndex, stickyGlobal) {
    if (variants.length === 1 && variants[0].checks.length === 0) {
        const eng = variants[0].engine;
        eng.lastIndex = stickyGlobal ? startIndex : 0;
        const m = __nfRegExpApply(__nfRegExpNativeExec, eng, [text]);
        return m ? { match: __nfAttachNamedGroups(m), lastIndex: eng.lastIndex } : null;
    }
    let best = null;
    for (let pos = stickyGlobal ? startIndex : 0; pos <= text.length; pos++) {
        for (const v of variants) {
            const sticky = v.sticky;
            sticky.lastIndex = pos;
            const m = __nfRegExpApply(__nfRegExpNativeExec, sticky, [text]);
            if (!m) continue;
            const valid = v.checks.every(chk => {
                const val = chk.groupIdx !== undefined ? m[chk.groupIdx] : m.groups?.[chk.ref];
                return chk.takeYes ? val !== undefined : val === undefined;
            });
            if (valid) {
                best = { match: __nfAttachNamedGroups(m), lastIndex: sticky.lastIndex };
                return best;
            }
        }
    }
    return null;
}
function __nfRegExpEngineVariants(state, global) {
    const nf = state.nativeFlags.replace(/g/g, '') + (global ? 'g' : '');
    return __nfBuildVariants(state.compiledInfo, nf);
}
function __nfRegExpExec(value, input) {
    const state = __nfRegExpState(value), text = __nfRegExpText(input == null ? 'null' : input, 'input');
    if (state.global && (state.lastIndex < 0 || state.lastIndex > text.length)) { state.lastIndex = 0; return null; }
    const res = __nfExecVariants(state.variants, text, state.global ? state.lastIndex : 0, state.global);
    if (!res) {
        if (state.global) state.lastIndex = 0;
        return null;
    }
    if (state.global) state.lastIndex = res.lastIndex;
    return res.match;
}
function __nfRegExpCreate(as3, args) {
    if (!as3) __nfRegExpUnsupported('RegExp is not an AS2 built-in.');
    if (args.length > 2) __nfRegExpUnsupported('constructor expects a pattern and optional flags.');
    let source, flags;
    if (__nfRegExpIs(args[0])) {
        if (args[1] !== undefined) throw new __nfTypeError('Cloning RegExp does not accept replacement flags.');
        const previous = __nfRegExpState(args[0]); source = previous.source; flags = previous.flags;
    } else {
        source = args[0] === undefined ? '' : __nfRegExpText(args[0], 'pattern', 65536);
        flags = args[1] === undefined ? '' : __nfRegExpText(args[1], 'flags', 64);
    }
    if (/[^gimsx]/.test(flags)) __nfRegExpUnsupported('flags must contain only g, i, m, s and x; unsupported flags are not silently ignored.');
    flags = [...new Set(flags)].join('');
    const initialFlags = { i: flags.includes('i'), m: flags.includes('m'), s: flags.includes('s'), x: flags.includes('x') };
    const compiledInfo = __nfCompilePcre(source, initialFlags);
    const nativeFlags = flags.replace(/x/g, '');
    let variants;
    try { variants = __nfBuildVariants(compiledInfo, nativeFlags); }
    catch (error) { __nfRegExpSyntaxError(error.message); }
    const state = {
        version: 1,
        source,
        flags,
        compiledInfo,
        nativeFlags,
        variants,
        global: flags.includes('g'),
        lastIndex: 0,
    };
    const value = Object.create(__nfRegExpClass(true).prototype);
    Object.defineProperty(value, __nfRegExpBrand, { value: state });
    Object.defineProperties(value, {
        source: { get: () => state.source },
        global: { get: () => state.global },
        ignoreCase: { get: () => flags.includes('i') },
        multiline: { get: () => flags.includes('m') },
        dotall: { get: () => flags.includes('s') },
        extended: { get: () => flags.includes('x') },
        lastIndex: { get: () => state.lastIndex, set: next => { state.lastIndex = __nfNumber(next); } },
        exec: { value: input => __nfRegExpExec(value, input), writable: true, configurable: true },
        test: { value: input => __nfRegExpExec(value, input) !== null, writable: true, configurable: true },
        toString: { value: () => '/' + state.source + '/' + state.flags, writable: true, configurable: true },
    });
    return value;
}
function __nfRegExpClass(as3) {
    if (!as3) __nfRegExpUnsupported('RegExp is not an AS2 built-in.');
    if (!__nfRegExpFacadeClass) {
        const facade = function(...args) {
            if (!new.target && __nfRegExpIs(args[0]) && args[1] === undefined) return args[0];
            return __nfRegExpCreate(true, args);
        };
        Object.defineProperties(facade, {
            __nfIs: { value: __nfRegExpIs },
            [Symbol.hasInstance]: { value: __nfRegExpIs },
            __nfInitialize: { value: () => __nfRegExpUnsupported('native subclass initialization is pending.') },
        });
        __nfRegExpFacadeClass = facade;
    }
    return __nfRegExpFacadeClass;
}
function __nfRegExpForString(pattern) {
    return __nfRegExpIs(pattern) ? pattern : __nfRegExpCreate(true, [pattern]);
}
function __nfRegExpMatch(text, pattern) {
    if (pattern === undefined) return null;
    const value = __nfRegExpForString(pattern), state = __nfRegExpState(value);
    const variants = __nfRegExpEngineVariants(state, state.global);
    if (!state.global) {
        const res = __nfExecVariants(variants, text, 0, false);
        return res ? res.match : null;
    }
    const result = []; let total = 0, cursor = 0;
    try {
        for (;;) {
            if (cursor > text.length) break;
            const res = __nfExecVariants(variants, text, cursor, true);
            if (!res) break;
            const match = res.match;
            total += match[0].length;
            if (result.length >= __nfRegExpLimit || total > __nfRegExpLimit) __nfRegExpUnsupported('match output exceeds the supported collection/string size.');
            result.push(match[0]);
            cursor = !match[0].length ? match.index + 1 : res.lastIndex;
        }
        return result;
    } finally { state.lastIndex = 0; }
}
function __nfRegExpSearch(text, pattern) {
    if (pattern === undefined) return -1;
    const state = __nfRegExpState(__nfRegExpForString(pattern));
    const res = __nfExecVariants(__nfRegExpEngineVariants(state, false), text, 0, false);
    return res ? res.match.index : -1;
}
function __nfRegExpReplacement(template, match, text) {
    let result = '', cursor = 0;
    const tokens = /\$(\$|&|`|'|<([^>]+)>|\d{1,2})/g;
    const append = part => {
        if (result.length + part.length > __nfRegExpLimit) __nfRegExpUnsupported('replace output exceeds 1000000 UTF-16 code units.');
        result += part;
    };
    for (;;) {
        const found = __nfRegExpApply(__nfRegExpNativeExec, tokens, [template]);
        if (!found) break;
        append(template.slice(cursor, found.index));
        cursor = found.index + found[0].length;
        const code = found[1];
        let part = found[0];
        if (code === '$') part = '$';
        else if (code === '&') part = match[0];
        else if (code === '`') part = text.slice(0, match.index);
        else if (code === "'") part = text.slice(match.index + match[0].length);
        else if (found[2] !== undefined) {
            part = match.groups && Object.hasOwn(match.groups, found[2]) ? (match.groups[found[2]] ?? '') : '';
        } else {
            const index = Number(code), first = Number(code[0]);
            if (index > 0 && index < match.length) part = match[index] === undefined ? '' : match[index];
            else if (code.length === 2 && first > 0 && first < match.length) part = (match[first] === undefined ? '' : match[first]) + code[1];
        }
        append(part);
    }
    append(template.slice(cursor));
    return result;
}
function __nfRegExpReplace(text, value, replacement) {
    const state = __nfRegExpState(value), variants = __nfRegExpEngineVariants(state, state.global), callback = typeof replacement === 'function';
    const template = callback ? null : __nfRegExpText(replacement, 'replacement');
    let result = '', cursor = 0, searchAt = 0, count = 0;
    for (;;) {
        if (searchAt > text.length) break;
        const res = __nfExecVariants(variants, text, searchAt, state.global);
        if (!res) break;
        const match = res.match;
        if (++count > __nfRegExpLimit) __nfRegExpUnsupported('replace match count exceeds the supported collection size.');
        const cbArgs = match.groups ? [...match, match.index, text, match.groups] : [...match, match.index, text];
        const inserted = callback ? __nfRegExpText(__nfRegExpApply(replacement, undefined, cbArgs), 'replacement callback result') : __nfRegExpReplacement(template, match, text);
        if (result.length + match.index - cursor + inserted.length > __nfRegExpLimit) __nfRegExpUnsupported('replace output exceeds 1000000 UTF-16 code units.');
        result += text.slice(cursor, match.index) + inserted;
        cursor = match.index + match[0].length;
        if (!state.global) break;
        searchAt = !match[0].length ? match.index + 1 : res.lastIndex;
    }
    if (result.length + text.length - cursor > __nfRegExpLimit) __nfRegExpUnsupported('replace output exceeds 1000000 UTF-16 code units.');
    return result + text.slice(cursor);
}
function __nfRegExpSplit(text, value, limit) {
    if (limit === 0) return [];
    if (!text.length) return [''];
    const state = __nfRegExpState(value), variants = __nfRegExpEngineVariants(state, true), result = [];
    let cursor = 0, searchAt = 0, total = 0;
    const push = item => {
        total += item === undefined ? 0 : item.length;
        if (result.length >= __nfRegExpLimit || total > __nfRegExpLimit) __nfRegExpUnsupported('split output exceeds the supported collection/string size.');
        result.push(item);
        return result.length >= limit;
    };
    for (;;) {
        if (searchAt > text.length) break;
        const res = __nfExecVariants(variants, text, searchAt, true);
        if (!res) break;
        const match = res.match, end = match.index + match[0].length;
        if (!match[0].length && (end === cursor || end === text.length)) { searchAt = match.index + 1; continue; }
        if (push(text.slice(cursor, match.index))) return result;
        for (let index = 1; index < match.length; index++) if (push(match[index])) return result;
        cursor = end;
        searchAt = !match[0].length ? match.index + 1 : res.lastIndex;
    }
    push(text.slice(cursor));
    return result;
}
