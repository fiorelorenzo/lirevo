import App from './App.svelte';
import { mount } from 'svelte';

const target = document.getElementById('app');
if (!target) throw new Error('missing #app root element');
mount(App, { target });
