const translations = [...document.querySelectorAll('[data-zh]')]
  .map(node => ({ node, english: node.innerHTML }));
const image = document.querySelector('[data-alt-zh]');
const imageAlt = image.alt;
const languageButton = document.getElementById('language');
const query = new URLSearchParams(location.search);
let chinese = query.get('lang') === 'zh'
  || (!query.has('lang') && navigator.language.toLowerCase().startsWith('zh'));

function applyLanguage() {
  document.documentElement.lang = chinese ? 'zh-Hans' : 'en';
  document.title = chinese ? 'XRack · 原生桌面 UI' : 'XRack · Native desktop UI';
  for (const { node, english } of translations) {
    if (chinese) node.textContent = node.dataset.zh;
    else node.innerHTML = english;
  }
  image.alt = chinese ? image.dataset.altZh : imageAlt;
  languageButton.lang = chinese ? 'en' : 'zh-Hans';
  languageButton.textContent = chinese ? 'English' : '中文';
  languageButton.setAttribute('aria-label', chinese ? 'Switch to English' : '切换到中文');
}

languageButton.addEventListener('click', () => {
  chinese = !chinese;
  const url = new URL(location.href);
  url.searchParams.set('lang', chinese ? 'zh' : 'en');
  history.replaceState(null, '', url);
  applyLanguage();
});
applyLanguage();

let count = 0;
const countOutput = document.getElementById('count');
const doubledOutput = document.getElementById('doubled');
const increment = document.getElementById('increment');
const reset = document.getElementById('reset');
function renderCounter() {
  countOutput.value = count;
  doubledOutput.value = count * 2;
}
increment.disabled = false;
reset.disabled = false;
increment.addEventListener('click', () => {
  count += 1;
  renderCounter();
});
reset.addEventListener('click', () => {
  count = 0;
  renderCounter();
});

const copy = document.getElementById('copy');
const copyStatus = document.getElementById('copy-status');
copy.disabled = !navigator.clipboard;
copy.addEventListener('click', async () => {
  try {
    await navigator.clipboard.writeText(document.getElementById('commands').textContent);
    copyStatus.textContent = chinese ? '命令已复制' : 'Commands copied.';
  } catch {
    copyStatus.textContent = chinese ? '请选择上方命令并手动复制。' : 'Select the commands above to copy them.';
  }
});
