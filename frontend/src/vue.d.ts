declare module '*.vue' {
  import type { DefineComponent } from 'vue';
  const component: DefineComponent;
  export default component;
}

declare const __SONICBOOM_BACKEND_PORT__: string;
