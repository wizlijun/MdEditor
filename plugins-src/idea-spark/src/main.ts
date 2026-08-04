import { mount } from 'svelte'
import App from './App.svelte'

const target = document.getElementById('idea-spark-app')
if (!target) throw new Error('idea-spark-app root missing')
mount(App, { target })
