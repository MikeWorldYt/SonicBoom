import { createApp } from 'vue';
import App from './App.vue';
import type { PageData } from './types';
import './styles/tokens.css';
import './styles/ui.css';

const root = document.querySelector<HTMLElement>('#app');
if (!root?.dataset.page) throw new Error('Missing SonicBoom page data');
const data = JSON.parse(root.dataset.page) as PageData;
document.body.className = data.page === 'admin' ? 'admin' : data.page === 'login' || data.page === 'error' ? 'login' : data.page;
createApp(App, { data }).mount(root);
