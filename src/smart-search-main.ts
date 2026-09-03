import { mount } from 'svelte'
import SmartSearchApp from './SmartSearchApp.svelte'

const target = document.getElementById('smart-search-app')
if (!target) throw new Error('smart-search-app root missing')
mount(SmartSearchApp, { target })
