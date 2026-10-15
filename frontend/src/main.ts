import { createApp } from 'vue';
import Home from './pages/Home.vue';
import Setup from './pages/Setup.vue';
import Admin from './pages/Admin.vue';
import type { PageData } from './types';
import './home.css';
import './setup.css';
import './admin.css';

const root = document.querySelector<HTMLElement>('#app');
if (!root?.dataset.page) throw new Error('Missing SonicBoom page data');
const data = JSON.parse(root.dataset.page) as PageData;
document.body.className = data.page === 'admin' ? 'admin' : data.page === 'login' || data.page === 'error' ? 'login' : data.page;
const page = data.page === 'home' ? Home : data.page === 'setup' ? Setup : Admin;
createApp(page, { data }).mount(root);
