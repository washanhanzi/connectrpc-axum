import DefaultTheme from 'vitepress/theme'
import ComparisonMeta from './ComparisonMeta.vue'
import './custom.css'

export default {
  extends: DefaultTheme,
  enhanceApp({ app }) {
    app.component('ComparisonMeta', ComparisonMeta)
  }
}
