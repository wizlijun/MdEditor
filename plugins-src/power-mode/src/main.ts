import { mount } from 'svelte'
import App from './App.svelte'

const target = document.getElementById('power-mode-app')
if (!target) throw new Error('power-mode-app root missing')
mount(App, { target })
