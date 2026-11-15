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

    function values(rows) {
        const list = element('dl');
        for (const property of rows) {
            const label = element('dt', property.name);
            label.append(element('small', property.kind));
            const value = element('dd');
            if (property.error) value.textContent = property.error;
            else if (Array.isArray(property.items)) {
                value.textContent = property.count + (property.count === 1 ? ' row' : ' rows');
                if (property.items.length) {
                    const items = element('ol');
                    for (const item of property.items) items.append(element('li', item));
                    value.append(items);
                }
                if (property.count > property.items.length) {
                    value.append(element('small', '+' + (property.count - property.items.length) + ' more'));
                }
            } else value.textContent = Object.hasOwn(property, 'value') ? String(property.value) : '—';
            list.append(label, value);
        }
        return list;
    }

    function select() {
        const nodes = snapshot.tree?.nodes ?? [];
        const node = nodes.find(node => node.key === selected) ?? nodes[0];
        for (const [key, button] of buttons) button.setAttribute('aria-pressed', String(key === node?.key));
        byId('property-title').textContent = node && node.parent !== null
            ? (node.name ?? node.kind) + ' properties' : 'Properties';
        source.hidden = !node?.source;
        componentSource.hidden = !node?.componentSource;
        const groups = [];
        const component = node?.parent == null ? snapshot.properties : node.componentProperties;
        if (component?.length) groups.push(['Component properties', component]);
        if (node?.properties.length) groups.push([node.kind + ' properties', node.properties]);
        const signature = JSON.stringify(groups);
        if (signature === propertyState) return;
        propertyState = signature;
        const content = document.createDocumentFragment();
        for (const [label, rows] of groups) {
            if (groups.length > 1 || node?.parent == null) content.append(element('h3', label));
            content.append(values(rows));
        }
        properties.replaceChildren(content);
    }

    function open(component) {
        const nodes = snapshot?.tree?.nodes ?? [];
        const node = nodes.find(node => node.key === selected) ?? nodes[0];
        if (node) vscode.postMessage({ type: 'source', key: node.key, component });
    }

    source.addEventListener('click', () => open(false));
    componentSource.addEventListener('click', () => open(true));
    pick.addEventListener('click', () => {
        vscode.postMessage({ type: 'pick', active: !snapshot.inspection.picking });
    });
    window.addEventListener('message', ({ data }) => {
        if (data?.type !== 'snapshot') return;
        snapshot = data;
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
        if (nativePick) buttons.get(selected)?.scrollIntoView({ block: 'nearest' });
    });
    vscode.postMessage({ type: 'ready' });
})();
