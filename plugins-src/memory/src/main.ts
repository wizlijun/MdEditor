import { mount } from 'svelte'
import App from './App.svelte'

const target = document.getElementById('memory-app')
if (!target) throw new Error('memory-app root missing')
mount(App, { target })
