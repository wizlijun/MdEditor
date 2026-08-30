import { mount } from 'svelte'
import App from './App.svelte'

const target = document.getElementById('next-app')
if (!target) throw new Error('next-app root missing')
mount(App, { target })
