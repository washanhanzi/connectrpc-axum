<script setup>
import { computed } from 'vue'
import { useData } from 'vitepress'

const { frontmatter } = useData()

const formattedDate = computed(() => {
  if (!frontmatter.value.date) return null
  const date = new Date(frontmatter.value.date)
  return date.toISOString().split('T')[0]
})
</script>

<template>
  <div v-if="frontmatter.repo" class="comparison-meta">
    <div class="meta-item">
      <span class="label">Repository:</span>
      <a :href="frontmatter.repo" target="_blank" rel="noopener">{{ frontmatter.repo }}</a>
    </div>
    <div class="meta-item" v-if="frontmatter.commit">
      <span class="label">Commit:</span>
      <code>{{ frontmatter.commit.slice(0, 7) }}</code>
    </div>
    <div class="meta-item" v-if="formattedDate">
      <span class="label">Date:</span>
      <span>{{ formattedDate }}</span>
    </div>
    <div class="meta-item" v-if="frontmatter.author">
      <span class="label">Author:</span>
      <span>{{ frontmatter.author }}</span>
    </div>
  </div>
</template>

<style scoped>
.comparison-meta {
  display: flex;
  flex-direction: column;
  gap: 0.5rem;
  padding: 1rem;
  margin-bottom: 1.5rem;
  background: var(--vp-c-bg-soft);
  border-radius: 8px;
  border: 1px solid var(--vp-c-divider);
  font-size: 0.9rem;
}

.meta-item {
  display: flex;
  align-items: center;
  gap: 0.5rem;
}

.label {
  min-width: 80px;
  font-weight: 600;
  color: var(--vp-c-text-2);
}

.comparison-meta a {
  color: var(--vp-c-brand-1);
  text-decoration: none;
}

.comparison-meta a:hover {
  text-decoration: underline;
}

.comparison-meta code {
  font-family: var(--vp-font-family-mono);
  font-size: 0.85em;
  padding: 2px 6px;
  background: var(--vp-c-bg-mute);
  border-radius: 4px;
}
</style>
