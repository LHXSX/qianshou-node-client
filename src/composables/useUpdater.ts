import { ref, onMounted } from "vue"
import { invoke } from "@tauri-apps/api/core"

export interface UpdateInfo {
  available: boolean
  version?: string
  notes?: string
  pub_date?: string
  download_url?: string
}

const _updateInfo = ref<UpdateInfo | null>(null)
const _checking = ref(false)
const _checkError = ref<string | null>(null)
const _installing = ref(false)
const _dismissed = ref(false)

export function useUpdater() {
  async function check() {
    _checking.value = true
    _checkError.value = null
    try {
      const info = await invoke<UpdateInfo>("check_for_updates")
      _updateInfo.value = info
    } catch (e: any) {
      _checkError.value = String(e)
      _updateInfo.value = null
    } finally {
      _checking.value = false
    }
  }

  async function install() {
    if (!_updateInfo.value?.available) return
    _installing.value = true
    try {
      await invoke("install_update")
      // 安装完会自动 restart；不会回到这里
    } catch (e: any) {
      _checkError.value = `安装失败: ${e}`
    } finally {
      _installing.value = false
    }
  }

  function dismiss() {
    _dismissed.value = true
  }

  // 启动后 5s 检查一次
  onMounted(() => {
    setTimeout(() => {
      check().catch(() => {})
    }, 5000)
  })

  return {
    updateInfo: _updateInfo,
    checking: _checking,
    checkError: _checkError,
    installing: _installing,
    dismissed: _dismissed,
    check,
    install,
    dismiss,
  }
}
