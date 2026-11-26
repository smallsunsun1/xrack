(() => {
    const vscode = acquireVsCodeApi();
    const byId = id => document.getElementById(id);
    const tree = byId('tree');
    const properties = byId('properties');
    const source = byId('source');
    const componentSource = byId('component-source');
    const pick = byId('pick');
    const saved = vscode.getState() ?? {};
    const collapsed = new Set(saved.collapsed);
    let selected = saved.selected ?? 'root';
    let snapshot;
    let structure;
    let propertyState;
    let buttons = new Map();
    let pickRevision;
    let callsState;
    let selectedCall;
    let updateState;
    let diagnosticState;

    function element(tag, text, className) {
        const node = document.createElement(tag);
        if (text !== undefined) node.textContent = text;
        if (className) node.className = className;
        return node;
    }

    function saveState() {
        vscode.setState({ selected, collapsed: [...collapsed] });
    }

    function showTree(nodes) {
        const levels = nodes.map(({ key, parent, kind, name }) => ({ key, parent, kind, name }));
        const signature = JSON.stringify(levels);
        if (signature === structure) return;
        structure = signature;
        buttons = new Map();
        const lists = new Map();
        const parents = new Set(levels.map(node => node.parent));
        const fragment = document.createDocumentFragment();
        for (const [index, node] of levels.entries()) {
            const li = element('li');
            const row = element('div', undefined, 'tree-row');
            const children = parents.has(index) ? element('ul') : undefined;
            if (children) {
                const toggle = element('button', undefined, 'toggle');
                toggle.type = 'button';
                const update = () => {
                    const open = !collapsed.has(node.key);
                    toggle.textContent = open ? '▾' : '▸';
                    toggle.setAttribute('aria-expanded', String(open));
                    toggle.setAttribute('aria-label', (open ? 'Collapse ' : 'Expand ') + (node.name ?? node.kind));
                    children.hidden = !open;
                };
                toggle.addEventListener('click', () => {
                    if (collapsed.has(node.key)) collapsed.delete(node.key);
                    else collapsed.add(node.key);
                    update();
                    saveState();
                });
                update();
                row.append(toggle);
                lists.set(index, children);
            }
            const button = element('button', undefined, 'select');
            button.type = 'button';
            button.append(element('span', node.parent === null ? snapshot.app : node.name ?? node.kind));
            if (node.name || node.parent === null) button.append(element('small', node.kind));
            button.addEventListener('click', () => {
                selected = node.key;
                select();
                saveState();
                vscode.postMessage({ type: 'select', key: node.key });
            });
            buttons.set(node.key, button);
            row.append(button);
            li.append(row);
            if (children) li.append(children);
            (lists.get(node.parent) ?? fragment).append(li);
        }
        tree.replaceChildren(fragment);
    }

    function openButton(label, message) {
        const button = element('button', label, 'source-link');
        button.type = 'button';
        button.addEventListener('click', () => vscode.postMessage({ type: 'source', ...message }));
        return button;
    }

    function values(rows, nodeKey) {
        const list = element('dl');
        for (const property of rows) {
            const label = element('dt', property.name);
            label.append(element('small', [property.kind, property.visibility].filter(Boolean).join(' · ')));
            if (nodeKey && property.source) label.append(openButton('Definition', { key: nodeKey, property: property.name }));
            const value = element('dd');
            if (property.error) value.textContent = property.error;
            else if (Array.isArray(property.fields)) {
                value.append(values(property.fields));
                if (property.truncated) value.append(element('small', 'More fields omitted'));
            } else if (Array.isArray(property.items)) {
                value.textContent = property.count + (property.count === 1 ? ' row' : ' rows');
                if (property.items.length) {
                    const items = element('ol');
                    for (const item of property.items) {
                        const row = element('li', Array.isArray(item) ? undefined : String(item));
                        if (Array.isArray(item)) row.append(values(item));
                        items.append(row);
                    }
                    value.append(items);
                }
                if (property.count > property.items.length) {
                    value.append(element('small', '+' + (property.count - property.items.length) + ' more'));
                }
            } else value.textContent = Object.hasOwn(property, 'value') ? String(property.value) : (property.status ?? '—');
            if (property.alias) value.append(element('small', 'Linked to ' + property.alias, 'value-meta'));
            list.append(label, value);
        }
        return list;
    }

    function showCalls(trace) {
        byId('calls-section').hidden = !trace;
        if (!trace) return;
        const signature = JSON.stringify(trace);
        if (signature === callsState) return;
        callsState = signature;
        const frames = trace.frames ?? [];
        const selector = byId('calls');
        selector.replaceChildren(...frames.slice().sort((a, b) => b.id - a.id).map(frame => {
            const option = element('option', '#' + frame.id + ' ' + frame.component + '.' + frame.name);
            option.value = String(frame.id);
            return option;
        }));
        byId('calls-picker').hidden = !frames.length;
        byId('calls-status').textContent = frames.length
            ? (trace.captureValues ? '' : 'Launch with XRACK_DEBUG_VALUES=1 to capture argument and return values.')
            : 'Interact with the running app to record calls.';
        renderStack();
    }

    function renderStack() {
        const trace = snapshot.trace;
        const frames = trace?.frames ?? [];
        const byFrame = new Map(frames.map(frame => [frame.id, frame]));
        const calls = byId('calls');
        let frame = byFrame.get(selectedCall) ?? byFrame.get(Number(calls.options[0]?.value));
        calls.value = frame ? String(frame.id) : '';
        const stack = [];
        for (; frame; frame = byFrame.get(frame.parentId)) {
            stack.unshift(frame);
            if (frame.parentId >= frame.id) break;
        }
        const content = document.createDocumentFragment();
        if (stack[0]?.parentId != null) content.append(element('p', 'Earlier caller no longer retained.'));
        for (const frame of stack) {
            const card = element('div', undefined, 'call-frame');
            const title = frame.component + '.' + frame.name;
            card.append(element('h3', title));
            const prior = frame.generation !== trace.generation;
            card.append(element('small', frame.status + ' · ' + (frame.elapsedMicros / 1000).toFixed(2) + ' ms'
                + (prior ? ' · Earlier source version' : '')));
            for (const [label, source, callSite] of [
                ['Called at', frame.callSite, true], ['Defined at', frame.source, false],
            ]) {
                if (!source) continue;
                const line = element('p', undefined, 'frame-location');
                const location = source.file.split(/[/\\]/).at(-1) + ':' + source.line + ':' + source.column;
                if (prior) line.textContent = label + ' ' + location;
                else {
                    const button = openButton(label + ' ' + location, { frame: frame.id, callSite });
                    button.title = source.file;
                    line.append(button);
                }
                card.append(line);
            }
            if (frame.error) card.append(element('p', frame.error, 'call-error'));
            const rows = [...(frame.parameters ?? [])];
            if (frame.result) rows.push({ name: 'Return', ...frame.result });
            if (rows.length) card.append(values(rows));
            if (frame.parametersTruncated) card.append(element('small', 'More parameters omitted'));
            content.append(card);
        }
        byId('call-stack').replaceChildren(content);
    }

    byId('calls').addEventListener('change', event => {
        selectedCall = Number(event.target.value);
        renderStack();
    });

    function requestIds(event) {
        if (Number.isSafeInteger(event.request_id)) return [String(event.request_id)];
        return typeof event.requests === 'string' ? event.requests.match(/\d+/g) ?? [] : [];
    }

    function showDiagnostics(records) {
        const events = Array.isArray(records) ? records.filter(record => typeof record?.event === 'string') : [];
        const section = byId('diagnostics-section');
        section.hidden = !events.length;
        const selector = byId('diagnostic-request');
        const signature = JSON.stringify([events, selector.value]);
        if (diagnosticState === signature) return;
        diagnosticState = signature;
        const selected = selector.value;
        const requests = [...new Set(events.flatMap(requestIds))];
        const option = id => {
            const item = element('option', id || 'All requests');
            item.value = id;
            return item;
        };
        selector.replaceChildren(...['', ...requests].map(option));
        selector.value = requests.includes(selected) ? selected : '';
        const list = events.filter(event => !selector.value || requestIds(event).includes(selector.value));
        byId('diagnostic-events').replaceChildren(...list.slice().reverse().map(({ event, ...fields }) => {
            const item = element('li', event.replaceAll('_', ' '));
            item.append(values(Object.entries(fields).map(([name, value]) => ({ name, value }))));
            return item;
        }));
    }

    byId('diagnostic-request').addEventListener('change', () => showDiagnostics(snapshot?.diagnostics));

    function showUpdates(node) {
        const plan = snapshot.updatePlan;
        byId('update-section').hidden = !plan;
        if (!plan) return;
        const entries = plan.entries.map((entry, index) => ({ ...entry, index }))
            .filter(entry => node?.parent == null || (node.name && entry.element === node.name));
        const generation = snapshot.trace?.generation;
        const sample = snapshot.updates?.findLast(sample => sample.generation === generation);
        const signature = JSON.stringify([node?.name, entries, sample, generation]);
        if (signature === updateState) return;
        updateState = signature;

        const content = document.createDocumentFragment();
        if (sample) {
            const summary = [
                sample.propertyEvaluations + ' property evaluations',
                sample.propertyCacheHits + ' property cache hits',
                sample.layoutRuns + ' layouts',
                sample.layoutReuses + ' layout reuses',
                sample.materializedRows + ' rows materialized',
                (sample.cpuMicros / 1000).toFixed(2) + ' ms CPU',
            ];
            content.append(element('p', 'Latest window update #' + sample.id));
            content.append(element('p', summary.join(' · ')));
            const view = sample.viewRows?.find(view => view.id === node?.name);
            if (view) content.append(element('p', node.name + ' · ' + view.rows + ' rows materialized'));
            const reads = sample.visualBindingReads?.filter(read => read.id === node?.name) ?? [];
            if (reads.length) content.append(element('p', 'Visual binding reads · ' + reads
                .map(read => read.property + ' × ' + read.reads).join(', ')));
        }
        content.append(element('h3', 'Compiler plan'));
        for (const entry of entries.slice(0, 20)) {
            const item = element('p', undefined, 'frame-location');
            const label = [entry.element ?? entry.kind ?? entry.component, entry.property].filter(Boolean).join('.');
            const decisions = Object.entries(entry.decisions).map(([key, value]) => key + ' = ' + value).join(', ');
            if (entry.source) item.append(openButton(label, { planIndex: entry.index, generation }));
            else item.append(element('span', label));
            item.append(element('small', ' · ' + decisions));
            content.append(item);
        }
        if (entries.length > 20 || plan.truncated) content.append(element('small', 'More decisions available through the update plan.'));
        byId('update-details').replaceChildren(content);
    }

    function select() {
        const nodes = snapshot.tree?.nodes ?? [];
        const node = nodes.find(node => node.key === selected) ?? nodes[0];
        for (const [key, button] of buttons) button.setAttribute('aria-pressed', String(key === node?.key));
        byId('property-title').textContent = node && node.parent !== null
            ? (node.name ?? node.kind) + ' properties' : 'Properties';
        source.hidden = !node?.source;
        componentSource.hidden = !node?.componentSource;
        showUpdates(node);
        const groups = [];
        const component = node?.parent == null ? snapshot.properties : node.componentProperties;
        if (component?.length) groups.push(['Component properties', component, node?.key ?? 'root']);
        if (node?.properties.length) groups.push([node.kind + ' properties', node.properties]);
        const signature = JSON.stringify([node?.key, groups]);
        if (signature === propertyState) return;
        propertyState = signature;
        const content = document.createDocumentFragment();
        for (const [label, rows, nodeKey] of groups) {
            if (groups.length > 1 || node?.parent == null) content.append(element('h3', label));
            content.append(values(rows, nodeKey));
        }
        properties.replaceChildren(content);
    }

    function open(component) {
        const nodes = snapshot?.tree?.nodes ?? [];
        const node = nodes.find(node => node.key === selected) ?? nodes[0];
        if (node) vscode.postMessage({ type: 'source', key: node.key, component });
    }

    const copyRecording = byId('copy-recording');
    copyRecording.addEventListener('click', () => vscode.postMessage({ type: 'copyRecording' }));
    source.addEventListener('click', () => open(false));
    componentSource.addEventListener('click', () => open(true));
    pick.addEventListener('click', () => {
        vscode.postMessage({ type: 'pick', active: !snapshot.inspection.picking });
    });
    window.addEventListener('message', ({ data }) => {
        if (data?.type !== 'snapshot') return;
        snapshot = data;
        const recording = data.recording;
        copyRecording.hidden = !recording;
        copyRecording.disabled = !recording?.count;
        if (recording) copyRecording.textContent = 'Copy Recording (' + recording.count + (recording.truncated ? '+' : '') + ')';
        byId('app').textContent = data.app;
        byId('status').textContent = data.status;
        const error = byId('error');
        const diagnostic = data.error ?? '';
        if (diagnostic !== error.textContent) error.textContent = diagnostic;
        error.hidden = !diagnostic;
        const nodes = data.tree?.nodes ?? [];
        const inspection = data.inspection;
        pick.hidden = !inspection;
        pick.setAttribute('aria-pressed', String(inspection?.picking ?? false));
        pick.textContent = inspection?.picking ? 'Cancel Picking' : 'Pick in Window';
        const nativePick = inspection?.pickRevision !== pickRevision && inspection?.key
            && nodes.find(node => node.key === inspection.key);
        pickRevision = inspection?.pickRevision;
        if (nativePick) {
            selected = nativePick.key;
            for (let node = nativePick; node.parent != null; ) {
                node = nodes[node.parent];
                if (collapsed.delete(node.key)) structure = undefined;
            }
            saveState();
        }
        byId('tree-section').hidden = !nodes.length;
        byId('truncated').hidden = !data.tree?.truncated;
        showTree(nodes);
        select();
        showCalls(data.trace);
        showDiagnostics(data.diagnostics);
        const viewports = data.viewportStatistics;
        const viewportSection = byId('viewport-statistics-section');
        viewportSection.hidden = !viewports?.views?.length && !viewports?.gpu?.views?.length;
        if (!viewportSection.hidden) {
            const output = byId('viewport-statistics-data');
            const text = globalThis.formatViewportStatistics(viewports);
            if (text !== output.textContent) output.textContent = text;
        }
        if (nativePick) buttons.get(selected)?.scrollIntoView({ block: 'nearest' });
    });
    vscode.postMessage({ type: 'ready' });
})();
