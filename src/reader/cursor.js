// Linux build (0022): text cursor for controllers. A box marks one character; it moves
// by character, line or word, and a lookup starts at it as if the mouse pointed there.
// In paged mode, moving past the page asks the app to turn it ("cursor-edge"); in
// continuous mode the cursor scrolls into view.
window.hoshiCursor = (function () {
    const params = new URLSearchParams(location.search);
    const scanLength = Number(params.get('sl')) || 16;
    const continuous = location.pathname.endsWith('__continuous.html');
    const selection = window.hoshiSelection;
    const MAX_STEPS = 4000;

    let active = false;
    let current = null; // { node, offset }
    let pendingEdge = null;
    let goal = null; // distance along the line kept over several line moves
    let box = null;
    let checkFrame = 0;

    function post(message) {
        parent.postMessage(message, '*');
    }

    function vertical() {
        return selection.isVertical();
    }

    function charLength(node, offset) {
        const code = node.data.charCodeAt(offset);
        return code >= 0xd800 && code <= 0xdbff && offset + 1 < node.data.length ? 2 : 1;
    }

    function charAt(position) {
        return position.node.data.substr(position.offset, charLength(position.node, position.offset));
    }

    function rectOf(position) {
        const range = document.createRange();
        range.setStart(position.node, position.offset);
        range.setEnd(position.node, position.offset + charLength(position.node, position.offset));
        for (const rect of range.getClientRects()) {
            if (rect.width > 0 || rect.height > 0) {
                return rect;
            }
        }
        return null;
    }

    function visible(rect) {
        const x = (rect.left + rect.right) / 2;
        const y = (rect.top + rect.bottom) / 2;
        return x >= 0 && x <= window.innerWidth && y >= 0 && y <= window.innerHeight;
    }

    function walker(node) {
        const result = selection.createWalker(document.body);
        result.currentNode = node;
        return result;
    }

    /** Positions after (step 1) or before (step -1) `from`, in document order. */
    function* positions(from, step) {
        let node = from.node;
        let offset = from.offset;
        const walk = walker(node);
        for (let count = 0; count < MAX_STEPS; count++) {
            if (step > 0) {
                offset += charLength(node, offset);
                while (offset >= node.data.length) {
                    node = walk.nextNode();
                    if (!node) return;
                    offset = 0;
                }
            } else {
                offset -= 1;
                while (offset < 0) {
                    node = walk.previousNode();
                    if (!node) return;
                    offset = node.data.length - 1;
                }
                const code = node.data.charCodeAt(offset);
                if (offset > 0 && code >= 0xdc00 && code <= 0xdfff) offset -= 1;
            }
            yield { node, offset };
        }
    }

    /** Characters a lookup can start at, with their on-screen box. */
    function* characters(from, step) {
        for (const position of positions(from, step)) {
            if (selection.isScanBoundary(charAt(position))) continue;
            const rect = rectOf(position);
            if (rect) yield { position, rect };
        }
    }

    function usable(position) {
        return position.offset < position.node.data.length &&
            !selection.isScanBoundary(charAt(position)) && !!rectOf(position);
    }

    function textNodes() {
        const walk = selection.createWalker(document.body);
        const nodes = [];
        let node;
        while ((node = walk.nextNode())) nodes.push(node);
        return nodes;
    }

    /** The first (or last) character shown on screen. */
    function edgeCharacter(last) {
        const nodes = textNodes();
        if (last) nodes.reverse();
        const range = document.createRange();
        for (const node of nodes) {
            range.selectNodeContents(node);
            const bounds = range.getBoundingClientRect();
            if (!bounds.width && !bounds.height) continue;
            if (bounds.right < 0 || bounds.left > window.innerWidth ||
                bounds.bottom < 0 || bounds.top > window.innerHeight) continue;
            const offsets = [];
            for (let offset = 0; offset < node.data.length; offset += charLength(node, offset)) offsets.push(offset);
            if (last) offsets.reverse();
            for (const offset of offsets) {
                const position = { node, offset };
                if (selection.isScanBoundary(charAt(position))) continue;
                const rect = rectOf(position);
                if (rect && visible(rect)) return position;
            }
        }
        return null;
    }

    function cueCharacter() {
        const cue = document.querySelector('.hoshi-sasayaki-cue.hoshi-sasayaki-active');
        if (!cue) return null;
        const walk = selection.createWalker(cue);
        let node;
        while ((node = walk.nextNode())) {
            for (let offset = 0; offset < node.data.length; offset += charLength(node, offset)) {
                const position = { node, offset };
                const rect = usable(position) && rectOf(position);
                if (rect) return visible(rect) ? position : null;
            }
        }
        return null;
    }

    function render() {
        const rect = active && current?.node.isConnected ? rectOf(current) : null;
        if (!rect) {
            box?.remove();
            box = null;
            return;
        }
        if (!box) {
            box = document.createElement('div');
            box.className = 'hoshi-controller-cursor';
            box.style.cssText = 'position: fixed; pointer-events: none; z-index: 2147483647; box-sizing: border-box;' +
                'border: 2px solid var(--hoshi-cursor-color, #f59e0b); border-radius: 4px;';
            document.documentElement.appendChild(box);
        }
        box.style.left = `${rect.left - 2}px`;
        box.style.top = `${rect.top - 2}px`;
        box.style.width = `${rect.width + 4}px`;
        box.style.height = `${rect.height + 4}px`;
    }

    function reveal(rect) {
        const margin = Math.min(48, window.innerWidth / 6, window.innerHeight / 6);
        const scroller = document.body;
        if (rect.left < margin) scroller.scrollLeft += rect.left - margin;
        else if (rect.right > window.innerWidth - margin) scroller.scrollLeft += rect.right - (window.innerWidth - margin);
        if (rect.top < margin) scroller.scrollTop += rect.top - margin;
        else if (rect.bottom > window.innerHeight - margin) scroller.scrollTop += rect.bottom - (window.innerHeight - margin);
    }

    /** Moves to `target` (from characters()), turning the page when it is not shown. */
    function go(target, step) {
        if (!target) {
            if (!continuous) edge(step);
            return false;
        }
        if (continuous) {
            current = target.position;
            reveal(target.rect);
        } else if (visible(target.rect)) {
            current = target.position;
        } else {
            edge(step);
            return false;
        }
        render();
        return true;
    }

    function edge(step) {
        pendingEdge = step > 0 ? 'forward' : 'backward';
        post({ hoshi: 'cursor-edge', dir: pendingEdge });
    }

    function moveCharacter(step) {
        goal = null;
        if (!current) return edge(step);
        go(characters(current, step).next().value, step);
    }

    function geometry() {
        const isVertical = vertical();
        const block = (rect) => isVertical ? (rect.left + rect.right) / 2 : (rect.top + rect.bottom) / 2;
        const size = (rect) => Math.max(isVertical ? rect.width : rect.height, 4);
        return {
            inline: (rect) => isVertical ? (rect.top + rect.bottom) / 2 : (rect.left + rect.right) / 2,
            newLine: (a, b) => Math.abs(block(a) - block(b)) > Math.max(size(a), size(b)) * 0.6,
        };
    }

    /** The first character of the line holding `position`. */
    function lineStartOf(position, g) {
        let start = { position, rect: rectOf(position) };
        for (const candidate of characters(position, -1)) {
            if (g.newLine(candidate.rect, start.rect)) break;
            start = candidate;
        }
        return start;
    }

    /** Along the line starting at `start`, the character closest to `along` from its start. */
    function closestAlong(start, along, g) {
        let best = start;
        let previous = start.rect;
        for (const candidate of characters(start.position, 1)) {
            if (g.newLine(candidate.rect, previous)) break;
            const distance = g.inline(candidate.rect) - g.inline(start.rect);
            if (Math.abs(distance - along) < Math.abs(g.inline(best.rect) - g.inline(start.rect) - along)) best = candidate;
            if (distance > along) break;
            previous = candidate.rect;
        }
        return best;
    }

    function moveLine(step) {
        if (!current) return edge(step);
        const g = geometry();
        const here = rectOf(current);
        if (!here) return edge(step);
        // Where the cursor sits along its line, kept over several line moves like a text editor.
        const along = goal ?? g.inline(here) - g.inline(lineStartOf(current, g).rect);
        goal = along;

        // The first character of the next line, or of the previous one.
        let start = null;
        let crossed = false;
        let previous = here;
        for (const candidate of characters(current, step)) {
            if (g.newLine(candidate.rect, previous)) {
                if (step > 0) {
                    start = candidate;
                    break;
                }
                if (crossed) break;
                crossed = true;
            }
            if (crossed) start = candidate;
            previous = candidate.rect;
        }
        if (!start) return go(null, step);
        go(closestAlong(start, along, g), step);
    }

    function block(node) {
        const element = node.nodeType === Node.TEXT_NODE ? node.parentElement : node;
        return element?.closest('p, h1, h2, h3, h4, h5, h6, li, blockquote, td, div') || document.body;
    }

    /** Word starts of the block holding `position`, with the index of `position`. */
    function words(position) {
        const container = block(position.node);
        const walk = selection.createWalker(container);
        const map = [];
        let text = '';
        let index = -1;
        let node;
        while ((node = walk.nextNode())) {
            for (let offset = 0; offset < node.data.length; offset += charLength(node, offset)) {
                if (node === position.node && offset === position.offset) index = text.length;
                const char = node.data.substr(offset, charLength(node, offset));
                for (let i = 0; i < char.length; i++) map.push({ node, offset });
                text += char;
            }
        }
        const segmenter = new Intl.Segmenter('ja', { granularity: 'word' });
        const starts = [...segmenter.segment(text)]
            .filter((segment) => segment.isWordLike)
            .map((segment) => ({ index: segment.index, position: map[segment.index] }));
        return { starts, index };
    }

    function moveWord(step) {
        goal = null;
        if (!current) return edge(step);
        if (typeof Intl.Segmenter !== 'function') return moveCharacter(step);
        const { starts, index } = words(current);
        const target = step > 0
            ? starts.find((start) => start.index > index)
            : starts.findLast((start) => start.index < index);
        if (target && usable(target.position)) {
            return go({ position: target.position, rect: rectOf(target.position) }, step);
        }
        // Past the block: the first word of the next one, or the start of the word before.
        const next = characters(current, step).next().value;
        if (!next || step > 0) return go(next, step);
        const before = words(next.position);
        const start = before.starts.findLast((entry) => entry.index <= before.index);
        const position = start && usable(start.position) ? start.position : next.position;
        go({ position, rect: rectOf(position) }, step);
    }

    function lookup() {
        const rect = current && rectOf(current);
        if (!rect) return;
        selection.selectText((rect.left + rect.right) / 2, (rect.top + rect.bottom) / 2, scanLength);
    }

    function place(at) {
        goal = null;
        current = at === 'cue' ? (cueCharacter() || edgeCharacter(false)) : edgeCharacter(at === 'last');
        render();
    }

    function start(at) {
        active = true;
        pendingEdge = null;
        place(at || 'cue');
        post({ hoshi: 'cursor-state', active: true });
    }

    function stop() {
        active = false;
        current = null;
        pendingEdge = null;
        render();
        post({ hoshi: 'cursor-state', active: false });
    }

    // After a page turn (by the cursor or any other way), keep the cursor on screen.
    function check() {
        checkFrame = 0;
        if (!active) return;
        const rect = current?.node.isConnected ? rectOf(current) : null;
        if (rect && visible(rect)) {
            render();
            return;
        }
        const along = goal;
        place(pendingEdge === 'backward' ? 'last' : 'first');
        pendingEdge = null;
        // After a line move across the page, land on the new page's line at the same place.
        if (along !== null && current) {
            const g = geometry();
            const best = closestAlong(lineStartOf(current, g), along, g);
            if (visible(best.rect)) current = best.position;
            goal = along;
            render();
        }
    }

    document.addEventListener('scroll', () => {
        if (active && !checkFrame) checkFrame = requestAnimationFrame(check);
    }, true);
    window.addEventListener('resize', () => {
        if (active && !checkFrame) checkFrame = requestAnimationFrame(check);
    });

    window.addEventListener('message', (e) => {
        const m = e.data;
        if (m?.hoshi !== 'cursor') return;
        if (m.op === 'start') start(m.at);
        else if (m.op === 'stop') stop();
        else if (!active) return;
        else if (m.op === 'lookup') lookup();
        else if (m.op === 'move' || m.op === 'word') {
            const isVertical = vertical();
            const along = isVertical ? { up: -1, down: 1 } : { left: -1, right: 1 };
            const across = isVertical ? { right: -1, left: 1 } : { up: -1, down: 1 };
            if (m.op === 'word') moveWord(m.step);
            else if (m.dir in along) moveCharacter(along[m.dir]);
            else if (m.dir in across) moveLine(across[m.dir]);
            if (m.follow) lookup();
        }
    });

    return { start, stop, get active() { return active; } };
})();
