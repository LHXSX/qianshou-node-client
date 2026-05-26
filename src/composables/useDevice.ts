import { ref, onMounted } from "vue"
import { invoke } from "@tauri-apps/api/core"

export interface SystemInfo {
  device_name: string
  hostname: string
  os_name: string
  os_version: string
  kernel_version: string
  cpu_brand: string
  cpu_cores: number
  cpu_threads: number
  total_memory_mb: number
  arch: string
}

export interface DeviceOverview {
  device_name: string
  system: SystemInfo
}

const _device = ref<DeviceOverview | null>(null)

export function useDevice() {
  async function refresh() {
    try {
      _device.value = await invoke<DeviceOverview>("get_system_info")
    } catch (e) {
      console.error("get_system_info failed:", e)
    }
  }

  async function rename(name: string): Promise<string | null> {
    try {
      const d = await invoke<DeviceOverview>("set_device_name", { name })
      _device.value = d
      return null
    } catch (e: any) {
      return String(e)
    }
  }

  onMounted(() => {
    refresh().catch(() => {})
  })

  return { device: _device, refresh, rename }
}
