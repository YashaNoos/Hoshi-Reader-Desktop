//
//  selection.js
//  Hoshi Reader
//
//  Copyright © 2026 Manhhao.
//  SPDX-License-Identifier: GPL-3.0-or-later
//

// https://github.com/yomidevs/yomitan/blob/ddbe4a2c0bf778583b38962d4b0b85442dfa8f6a/ext/js/language/CJK-util.js#L19
const CJK_UNIFIED_IDEOGRAPHS_RANGE = [0x4e00, 0x9fff];
const CJK_UNIFIED_IDEOGRAPHS_EXTENSION_A_RANGE = [0x3400, 0x4dbf];
const CJK_UNIFIED_IDEOGRAPHS_EXTENSION_B_RANGE = [0x20000, 0x2a6df];
const CJK_UNIFIED_IDEOGRAPHS_EXTENSION_C_RANGE = [0x2a700, 0x2b73f];
const CJK_UNIFIED_IDEOGRAPHS_EXTENSION_D_RANGE = [0x2b740, 0x2b81f];
const CJK_UNIFIED_IDEOGRAPHS_EXTENSION_E_RANGE = [0x2b820, 0x2ceaf];
const CJK_UNIFIED_IDEOGRAPHS_EXTENSION_F_RANGE = [0x2ceb0, 0x2ebef];
const CJK_UNIFIED_IDEOGRAPHS_EXTENSION_G_RANGE = [0x30000, 0x3134f];
const CJK_UNIFIED_IDEOGRAPHS_EXTENSION_H_RANGE = [0x31350, 0x323af];
const CJK_UNIFIED_IDEOGRAPHS_EXTENSION_I_RANGE = [0x2ebf0, 0x2ee5f];
const CJK_COMPATIBILITY_IDEOGRAPHS_RANGE = [0xf900, 0xfaff];
const CJK_COMPATIBILITY_IDEOGRAPHS_SUPPLEMENT_RANGE = [0x2f800, 0x2fa1f];
const CJK_IDEOGRAPH_RANGES = [
    CJK_UNIFIED_IDEOGRAPHS_RANGE,
    CJK_UNIFIED_IDEOGRAPHS_EXTENSION_A_RANGE,
    CJK_UNIFIED_IDEOGRAPHS_EXTENSION_B_RANGE,
    CJK_UNIFIED_IDEOGRAPHS_EXTENSION_C_RANGE,
    CJK_UNIFIED_IDEOGRAPHS_EXTENSION_D_RANGE,
    CJK_UNIFIED_IDEOGRAPHS_EXTENSION_E_RANGE,
    CJK_UNIFIED_IDEOGRAPHS_EXTENSION_F_RANGE,
    CJK_UNIFIED_IDEOGRAPHS_EXTENSION_G_RANGE,
    CJK_UNIFIED_IDEOGRAPHS_EXTENSION_H_RANGE,
    CJK_UNIFIED_IDEOGRAPHS_EXTENSION_I_RANGE,
    CJK_COMPATIBILITY_IDEOGRAPHS_RANGE,
    CJK_COMPATIBILITY_IDEOGRAPHS_SUPPLEMENT_RANGE,
];

// https://github.com/yomidevs/yomitan/blob/ddbe4a2c0bf778583b38962d4b0b85442dfa8f6a/ext/js/language/CJK-util.js#L60
const FULLWIDTH_CHARACTER_RANGES = [
    [0xff10, 0xff19], // Fullwidth numbers
    [0xff21, 0xff3a], // Fullwidth upper case Latin letters
    [0xff41, 0xff5a], // Fullwidth lower case Latin letters
    
    [0xff01, 0xff0f], // Fullwidth punctuation 1
    [0xff1a, 0xff1f], // Fullwidth punctuation 2
    [0xff3b, 0xff3f], // Fullwidth punctuation 3
    [0xff5b, 0xff60], // Fullwidth punctuation 4
    [0xffe0, 0xffee], // Currency markers
];

// https://github.com/yomidevs/yomitan/blob/ddbe4a2c0bf778583b38962d4b0b85442dfa8f6a/ext/js/language/ja/japanese.js#L44
const JAPANESE_RANGES = [
    [0x3040, 0x309f], // Hiragana
    [0x30a0, 0x30ff], // Katakana
    
    ...CJK_IDEOGRAPH_RANGES, // CJK_IDEOGRAPH_RANGES
    
    [0xff66, 0xff9f], // Halfwidth katakana
    
    [0x30fb, 0x30fc], // Katakana punctuation
    [0xff61, 0xff65], // Kana punctuation
    
    [0x3000, 0x303f], // CJK_PUNCTUATION_RANGE
    ...FULLWIDTH_CHARACTER_RANGES, // FULLWIDTH_CHARACTER_RANGES
];

// Linux (WebKitGTK): the native selection highlight of a line that is not the first of its
// paragraph is stretched to reach the previous line (WebKit fills the gap between lines),
// so in vertical text the lookup highlight is much wider on inner lines. Keep the native
// selection but make it transparent, and draw the highlight from the characters' own
// boxes (range.getClientRects()) in a fixed layer on top (above book backgrounds; the
// 40% gray tints the glyphs only slightly).
const HOSHI_LOOKUP_OVERLAY = /Linux/.test(navigator.userAgent) && !/Android/.test(navigator.userAgent);
const hoshiLookupOverlay = {
    range: null,
    layer: null,
    show(range) {
        if (!this.layer) {
            const style = document.createElement('style');
            style.textContent = `
                html.hoshi-lookup-selection ::selection { background-color: transparent !important; }
                #hoshi-lookup-overlay { position: fixed; inset: 0; z-index: 2147483647; pointer-events: none;
                    writing-mode: horizontal-tb; }
                #hoshi-lookup-overlay > div { position: absolute; background-color: rgba(160, 160, 160, 0.4); }`;
            document.head.appendChild(style);
            this.layer = document.createElement('div');
            this.layer.id = 'hoshi-lookup-overlay';
            addEventListener('scroll', () => this.draw(), { capture: true, passive: true });
            addEventListener('resize', () => this.draw(), { passive: true });
        }
        if (!this.layer.isConnected) document.documentElement.appendChild(this.layer);
        this.range = range;
        document.documentElement.classList.add('hoshi-lookup-selection');
        this.draw();
    },
    draw() {
        if (!this.layer) return;
        this.layer.replaceChildren();
        if (!this.range) return;
        for (const rect of this.rects(this.range)) {
            if (rect.width < 1 || rect.height < 1) continue;
            const box = document.createElement('div');
            box.style.left = `${rect.left}px`;
            box.style.top = `${rect.top}px`;
            box.style.width = `${rect.width}px`;
            box.style.height = `${rect.height}px`;
            this.layer.appendChild(box);
        }
    },
    // Rects of the range's text, without furigana: <rt>/<rp> are not selectable, so the
    // native highlight skips them and so does this one.
    rects(range) {
        const root = range.commonAncestorContainer;
        if (root.nodeType === Node.TEXT_NODE) {
            return root.parentElement?.closest('rt, rp') ? [] : Array.from(range.getClientRects());
        }
        const rects = [];
        const walker = document.createTreeWalker(root, NodeFilter.SHOW_TEXT);
        for (let node = walker.nextNode(); node; node = walker.nextNode()) {
            if (!range.intersectsNode(node) || node.parentElement?.closest('rt, rp')) continue;
            const part = document.createRange();
            part.selectNodeContents(node);
            if (node === range.startContainer) part.setStart(node, range.startOffset);
            if (node === range.endContainer) part.setEnd(node, range.endOffset);
            rects.push(...part.getClientRects());
        }
        return rects;
    },
    hide() {
        this.range = null;
        document.documentElement.classList.remove('hoshi-lookup-selection');
        this.layer?.replaceChildren();
    },
    // the user selected something else (e.g. by dragging): show that selection normally
    onSelectionChange(selection) {
        if (!this.range) return;
        const current = selection && selection.rangeCount ? selection.getRangeAt(0) : null;
        if (!current || current.compareBoundaryPoints(Range.START_TO_START, this.range) !== 0 ||
            current.compareBoundaryPoints(Range.END_TO_END, this.range) !== 0) {
            this.hide();
        }
    },
};

window.hoshiSelection = {
    selection: null,
    scanDelimiters: '。、！？…‥「」『』（）()【】〈〉《》〔〕｛｝{}［］[]：；:;，,.─\n\r',
    sentenceDelimiters: '。！？.!?\n\r',
    trailingSentenceChars: '。、！？」』）)】〉》〕｝}］]',
    brackets: {'「':'」', '『': '』', '（':'）', '(':')', '【':'】', '〈':'〉', '《':'》', '〔':'〕', '｛':'｝', '{':'}', '［':'］', '[':']'},
    
    isVertical() {
        return window.getComputedStyle(document.body).writingMode === "vertical-rl";
    },
    
    // https://github.com/yomidevs/yomitan/blob/ddbe4a2c0bf778583b38962d4b0b85442dfa8f6a/ext/js/language/ja/japanese.js#L307
    isCodePointJapanese(codePoint) {
        return JAPANESE_RANGES.some(([start, end]) => codePoint >= start && codePoint <= end);
    },
    
    isScanBoundary(char) {
        return /^[\s\u3000]$/.test(char) ||
        this.scanDelimiters.includes(char) ||
        (window.scanNonJapaneseText === false && !this.isCodePointJapanese(char.codePointAt(0)));
    },
    
    isFurigana(node) {
        const el = node.nodeType === Node.TEXT_NODE ? node.parentElement : node;
        return !!el?.closest('rt, rp');
    },
    
    revealFurigana(ruby) {
        const group = [ruby];
        for (const direction of ['previousSibling', 'nextSibling']) {
            let node = ruby[direction];
            while (node && (node.localName === 'ruby' ||
                            (node.nodeType === Node.TEXT_NODE && /^[\t\n\r ]*$/.test(node.nodeValue)))) {
                if (node.localName === 'ruby') {
                    group.push(node);
                }
                node = node[direction];
            }
        }
        group.forEach(el => el.classList.remove('furigana-hidden'));
    },
    
    findParagraph(node) {
        let el = node.nodeType === Node.TEXT_NODE ? node.parentElement : node;
        return el?.closest('p, .glossary-content, .expr-tag, .kanji-entry li') || null;
    },
    
    createWalker(rootNode) {
        const root = rootNode || document.body;
        
        return document.createTreeWalker(root, NodeFilter.SHOW_TEXT, {
            acceptNode: (n) => this.isFurigana(n) ? NodeFilter.FILTER_REJECT : NodeFilter.FILTER_ACCEPT
        });
    },
    
    inCharRange(charRange, x, y) {
        const rects = charRange.getClientRects();
        if (rects.length) {
            for (const rect of rects) {
                if (x >= rect.left && x <= rect.right && y >= rect.top && y <= rect.bottom) {
                    return true;
                }
            }
            return false;
        }
        const rect = charRange.getBoundingClientRect();
        return x >= rect.left && x <= rect.right && y >= rect.top && y <= rect.bottom;
    },
    
    getCaretRange(x, y) {
        if (document.caretPositionFromPoint) {
            const pos = document.caretPositionFromPoint(x, y);
            if (!pos) {
                return null;
            }
            
            const range = document.createRange();
            range.setStart(pos.offsetNode, pos.offset);
            range.collapse(true);
            return range;
        } else {
            const element = document.elementFromPoint(x, y);
            if (!element) {
                return null;
            }
            
            const container = element.closest('p, div, span, ruby, a') || document.body;
            const walker = this.createWalker(container);
            
            const range = document.createRange();
            let node;
            while (node = walker.nextNode()) {
                for (let i = 0; i < node.textContent.length; i++) {
                    range.setStart(node, i);
                    range.setEnd(node, i + 1);
                    if (this.inCharRange(range, x, y)) {
                        range.collapse(true);
                        return range;
                    }
                }
            }
            return document.caretRangeFromPoint(x, y);
        }
    },
    
    getCharacterAtPoint(x, y) {
        if (document.elementFromPoint(x, y) === document.documentElement) {
            return null;
        }
        
        const range = this.getCaretRange(x, y);
        if (!range) {
            return null;
        }
        
        const node = range.startContainer;
        if (node.nodeType !== Node.TEXT_NODE) {
            return null;
        }
        
        if (this.isFurigana(node)) {
            return null;
        }
        
        const text = node.textContent;
        const caret = range.startOffset;
        
        for (const offset of [caret, caret - 1, caret + 1]) {
            if (offset < 0 || offset >= text.length) {
                continue;
            }
            
            const charRange = document.createRange();
            charRange.setStart(node, offset);
            charRange.setEnd(node, offset + 1);
            if (this.inCharRange(charRange, x, y)) {
                if (this.isScanBoundary(text[offset])) {
                    return null;
                }
                return { node, offset };
            }
        }
        
        return null;
    },
    
    getSentence(startNode, startOffset) {
        const container = this.findParagraph(startNode) || document.body;
        const walker = this.createWalker(container);
        
        walker.currentNode = startNode;
        const partsBefore = [];
        let node = startNode;
        let limit = startOffset;
        
        while (node) {
            const text = node.textContent;
            let foundStart = false;
            for (let i = limit - 1; i >= 0; i--) {
                if (this.sentenceDelimiters.includes(text[i])) {
                    partsBefore.push(text.slice(i + 1, limit));
                    foundStart = true;
                    break;
                }
            }
            
            if (foundStart) {
                break;
            }
            
            partsBefore.push(text.slice(0, limit));
            node = walker.previousNode();
            if (node) limit = node.textContent.length;
        }
        
        walker.currentNode = startNode;
        const partsAfter = [];
        node = startNode;
        let start = startOffset;
        
        while (node) {
            const text = node.textContent;
            let foundEnd = false;
            
            for (let i = start; i < text.length; i++) {
                if (this.sentenceDelimiters.includes(text[i])) {
                    let end = i + 1;
                    
                    while (end < text.length) {
                        if (!this.trailingSentenceChars.includes(text[end])) break;
                        end += 1;
                    }
                    partsAfter.push(text.slice(start, end));
                    foundEnd = true;
                    break;
                }
            }
            
            if (foundEnd) {
                break;
            }
            
            partsAfter.push(text.slice(start));
            
            node = walker.nextNode();
            start = 0;
        }
        
        const prefix = partsBefore.reverse().join('');
        const raw = prefix + partsAfter.join('');
        let sentence = raw.trim();

        const closeBrackets = new Set(Object.values(this.brackets));
        const openBrackets = new Set(Object.keys(this.brackets));
        let stack = [];
        let unmatchedClose = [];
        
        for (let i = 0; i < sentence.length; i++) {
            const ch = sentence[i];
            if (openBrackets.has(ch)) {
                stack.push(ch);
            } else if (closeBrackets.has(ch)) {
                if (stack.length > 0 && this.brackets[stack[stack.length-1]] === ch) {
                    stack.pop();
                } else {
                    unmatchedClose.push(ch);
                }
            }
        }
        
        let startSlice = 0;
        while (stack.length > 0 && startSlice < sentence.length - 1) {
            // Stack consists of unmatched open brackets arranged from start to end
            if (stack[0] === sentence[startSlice]) {
                stack.shift();
            } else break;
            startSlice++;
        }
        
        let endSlice = sentence.length - 1;
        let endIdx = sentence.length - 1;
        while (unmatchedClose.length > 0 && endIdx > startSlice) {
            if (unmatchedClose[unmatchedClose.length - 1] === sentence[endIdx]) {
                unmatchedClose.pop();
                endSlice = endIdx - 1;
                // sentenceDelimiters used as trailingSentenceDelimiters as it does not have any overlap with brackets
            } else if (!this.sentenceDelimiters.includes(sentence[endIdx])) break;
            endIdx--;
        }

        const sliced = sentence.slice(startSlice, endSlice + 1);
        const rawSelectionOffset = prefix.length;
        const rawSentenceStart =
        (raw.length - raw.trimStart().length) +
        startSlice +
        (sliced.length - sliced.trimStart().length);
        const clozeOffset = rawSelectionOffset - rawSentenceStart;
        return { sentence: sliced.trim(), clozeOffset };
    },
    
    selectText(x, y, maxLength, scanLinks = false) {
        const el = document.elementFromPoint(x, y);
        if (!scanLinks && el?.closest('a')) {
            return 'link'
        }
        if (el?.closest('img, image, .blur-wrapper')) {
            return 'image'
        }
        
        const furigana = el?.closest('ruby.furigana-hidden');
        if (furigana) {
            this.revealFurigana(furigana);
            this.clearSelection();
            return 'furigana';
        }
        
        const hit = this.getCharacterAtPoint(x, y);
        
        if (!hit) {
            return null;
        }

        if (this.selection &&
            hit.node === this.selection.startNode &&
            hit.offset === this.selection.startOffset) {
            return this.selection.text;
        }

        this.selection = null;

        const container = this.findParagraph(hit.node) || document.body;
        const walker = this.createWalker(container);
        
        let text = '';
        let node = hit.node;
        let offset = hit.offset;
        let ranges = [];
        
        walker.currentNode = node;
        while (text.length < maxLength && node) {
            const content = node.textContent;
            const start = offset;
            
            while (offset < content.length && text.length < maxLength) {
                const char = content[offset];
                if (this.isScanBoundary(char)) {
                    break;
                }
                text += char;
                offset++;
            }
            
            if (offset > start) {
                ranges.push({ node, start, end: offset });
            }
            
            if (offset < content.length || text.length >= maxLength) {
                break;
            }
            
            node = walker.nextNode();
            offset = 0;
        }
        
        if (!text) {
            return null;
        }
        
        this.selection = {
            startNode: hit.node,
            startOffset: hit.offset,
            ranges,
            text
        };
        
        const { sentence, clozeOffset } = this.getSentence(hit.node, hit.offset);
        const normalizedOffset = window.hoshiReader ? this.getNormalizedOffset(hit.node, hit.offset) : null;
        webkit.messageHandlers.textSelected.postMessage({
            text,
            sentence,
            rect: this.getSelectionRect(x, y),
            normalizedOffset,
            clozeOffset
        });
        
        return text;
    },
    
    getSelectionRect(x, y) {
        if (!this.selection?.ranges.length) {
            return null;
        }
        
        const first = this.selection.ranges[0];
        const range = document.createRange();
        range.setStart(first.node, first.start);
        range.setEnd(first.node, first.start + 1);
        
        const rects = Array.from(range.getClientRects());
        const rect = rects.find(rect => x >= rect.left && x <= rect.right && y >= rect.top && y <= rect.bottom) ?? range.getBoundingClientRect();
        return {
            x: rect.x,
            y: rect.y,
            width: rect.width,
            height: rect.height
        };
    },
    
    highlightSelection(charCount) {
        if (!this.selection?.ranges.length) {
            return;
        }
        
        const selection = window.getSelection();
        if (!selection) {
            return;
        }

        let startPoint = null;
        let endPoint = null;
        let remaining = charCount;

        for (const r of this.selection.ranges) {
            if (remaining <= 0) {
                break;
            }

            let endOffset = r.start;
            while (endOffset < r.end && remaining > 0) {
                const char = String.fromCodePoint(r.node.textContent.codePointAt(endOffset));
                endOffset += char.length;
                remaining--;
            }

            if (endOffset > r.start) {
                startPoint ??= { node: r.node, offset: r.start };
                endPoint = { node: r.node, offset: endOffset };
            }
        }

        if (startPoint && endPoint) {
            const range = document.createRange();
            range.setStart(startPoint.node, startPoint.offset);
            range.setEnd(endPoint.node, endPoint.offset);
            if (HOSHI_LOOKUP_OVERLAY) hoshiLookupOverlay.hide();
            selection.removeAllRanges();
            selection.addRange(range);
            if (HOSHI_LOOKUP_OVERLAY) hoshiLookupOverlay.show(range);
        }
    },
    
    getNormalizedOffset(targetNode, offset) {
        let count = window.hoshiReader.nodeStartOffsets.get(targetNode) ?? 0;
        const text = targetNode.textContent;
        for (let i = 0; i < offset;) {
            const char = String.fromCodePoint(text.codePointAt(i));
            if (window.hoshiReader.isMatchableChar(char)) {
                count++;
            }
            i += char.length;
        }
        return count;
    },
    
    clearSelection() {
        if (HOSHI_LOOKUP_OVERLAY) hoshiLookupOverlay.hide();
        window.getSelection()?.removeAllRanges();
        CSS.highlights?.get('hoshi-selection')?.clear();
        this.selection = null;
    }
};

let lastHasSelection = false;
document.addEventListener('selectionchange', () => {
    const s = getSelection();
    if (HOSHI_LOOKUP_OVERLAY) hoshiLookupOverlay.onSelectionChange(s);
    const hasSelection = !!s && !s.isCollapsed;
    if (hasSelection === lastHasSelection) return;
    lastHasSelection = hasSelection;
    try { window.webkit?.messageHandlers?.selectionState?.postMessage(hasSelection); } catch {}
});
