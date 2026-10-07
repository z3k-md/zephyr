import { createApp } from 'vue';
import App from './App.vue';
import './styles.css';

// Lets the styles follow the native window chrome, which differs between platforms.
if (navigator.userAgent.includes('Windows')) document.documentElement.dataset.platform = 'windows';

createApp(App).mount('#app');
