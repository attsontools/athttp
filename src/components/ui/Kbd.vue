<template>
  <kbd class="kbd"><slot /></kbd>
</template>

<style scoped>
.kbd {
  display: inline-flex;
  align-items: center;
  border: 1px solid var(--line-strong);
  border-radius: 3px;
  padding: 1px 5px;
  font-family: var(--font-mono);
  font-size: 10px;
  color: var(--text-faint);
  background: transparent;
  line-height: 1.3;
}
</style>
