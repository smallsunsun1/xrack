const filter = document.getElementById('control-filter');
const status = document.getElementById('search-status');
const controls = [...document.querySelectorAll('.control')].map(element => ({
  element,
  text: element.textContent.toLowerCase(),
}));

function applyFilter() {
  const terms = filter.value.trim().toLowerCase().split(/\s+/).filter(Boolean);
  let hits = 0;
  for (const { element, text } of controls) {
    element.hidden = !terms.every(term => text.includes(term));
    if (!element.hidden) hits += 1;
  }
  status.textContent = hits ? `显示 ${hits} 个内建元素` : '未找到匹配的控件';
}

function openLinkedControl() {
  const target = document.getElementById(location.hash.slice(1));
  if (target instanceof HTMLDetailsElement) {
    if (target.hidden) {
      filter.value = '';
      applyFilter();
    }
    target.open = true;
    target.scrollIntoView();
  }
}

filter.addEventListener('input', applyFilter);
window.addEventListener('hashchange', openLinkedControl);
applyFilter();
openLinkedControl();
