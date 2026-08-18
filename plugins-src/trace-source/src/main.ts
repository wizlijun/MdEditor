import { mount } from 'svelte'
import App from './App.svelte'

const target = document.getElementById('trace-source-app')
if (!target) throw new Error('trace-source-app root missing')
mount(App, { target })
