import { createApp } from 'vue';

import App from './App.vue';
import { host } from '@runtime-host';
import './styles/tokens.css';
import './styles/reset.css';
import './styles/app.css';

createApp(App, { host }).mount('#app');
