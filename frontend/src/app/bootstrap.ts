import { createApp } from 'vue';
import App from '../App.vue';
import { parsePageData } from './pageData';
import { t } from '../i18n';
export function bootstrap(root: HTMLElement | null) {
  function failed() {
    const target = root ?? document.body.appendChild(document.createElement('main'));
    target.replaceChildren();
    const heading = document.createElement('h1'), help = document.createElement('p'), reload = document.createElement('button');
    heading.textContent = t('The interface could not load.');
    help.textContent = t('Reload the page. If the problem continues, check the server connection and logs.');
    reload.textContent = t('Reload page'); reload.addEventListener('click', () => location.reload());
    target.setAttribute('role', 'alert'); target.append(heading, help, reload);
  }
  try {
    if (!root?.dataset.page) return failed();
    const data = parsePageData(root.dataset.page);
    delete root.dataset.page;
    document.body.className = data.page === 'admin' ? 'admin' : data.page === 'login' || data.page === 'error' ? 'login' : data.page;
    const app = createApp(App, { data });
    app.config.errorHandler = failed;
    app.mount(root);
  } catch { failed(); }
}
