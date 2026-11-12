import { createApp } from 'vue';

import App from './App.vue';
import { createStandaloneHost } from './frontend/standaloneHost';
import './styles/tokens.css';
import './styles/reset.css';
import './styles/app.css';

createApp(App, { host: createStandaloneHost() }).mount('#app');
