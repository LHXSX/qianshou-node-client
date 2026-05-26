import { ref, watch } from "vue"

export type Theme = "light" | "dark"

const STORAGE_KEY = "ec-theme"

function readStored(): Theme {
  try {
    const v = localStorage.getItem(STORAGE_KEY)
    if (v === "light" || v === "dark") return v
  } catch {}
  return "dark"
}

function apply(theme: Theme) {
  const html = document.documentElement
  html.setAttribute("data-theme", theme)
}

const _theme = ref<Theme>(readStored())

// 启动立即应用
apply(_theme.value)

watch(_theme, (t) => {
  apply(t)
  try { localStorage.setItem(STORAGE_KEY, t) } catch {}
})

export function useTheme() {
  function toggle() {
    _theme.value = _theme.value === "light" ? "dark" : "light"
  }
  function setTheme(t: Theme) {
    _theme.value = t
  }
  return { theme: _theme, toggle, setTheme }
}
