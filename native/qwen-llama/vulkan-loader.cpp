// Only the loader is opened; vendor drivers remain managed by the system.
#include <vulkan/vulkan.h>
#include <stdexcept>
#ifdef _WIN32
#define WIN32_LEAN_AND_MEAN
#include <windows.h>
#else
#include <dlfcn.h>
#endif
namespace {
void *library_handle() {
    // Retain for process lifetime: ggml's registry/dispatcher also lives that long.
    static void *library = []() -> void * {
#ifdef _WIN32
        return LoadLibraryExW(L"vulkan-1.dll", nullptr, LOAD_LIBRARY_SEARCH_SYSTEM32);
#else
        return dlopen("libvulkan.so.1", RTLD_NOW | RTLD_LOCAL);
#endif
    }();
    return library;
}
void *library_symbol(const char *name) {
    auto library = library_handle();
    if (!library) return nullptr;
#ifdef _WIN32
    return reinterpret_cast<void *>(GetProcAddress(static_cast<HMODULE>(library), name));
#else
    return dlsym(library, name);
#endif
}
PFN_vkGetInstanceProcAddr entry() {
    static auto proc = reinterpret_cast<PFN_vkGetInstanceProcAddr>(library_symbol("vkGetInstanceProcAddr"));
    return proc;
}
}
template <typename T> T resolve(const char *name) {
    // Direct library lookup: vkGetInstanceProcAddr(nullptr, ...) legitimately
    // returns null for instance/device commands, which would silently
    // disable device features.
    return reinterpret_cast<T>(library_symbol(name));
}
extern "C" bool sb_vulkan_loader_available() noexcept { return entry() != nullptr; }
// ggml-vulkan calls these three entry points directly (not via Vulkan-Hpp).
extern "C" VKAPI_ATTR PFN_vkVoidFunction VKAPI_CALL vkGetDeviceProcAddr(VkDevice device, const char *name) {
    static auto fn = resolve<PFN_vkGetDeviceProcAddr>("vkGetDeviceProcAddr");
    return fn ? fn(device, name) : nullptr;
}
extern "C" VKAPI_ATTR void VKAPI_CALL vkGetPhysicalDeviceFeatures2(VkPhysicalDevice device, VkPhysicalDeviceFeatures2 *features) {
    static auto fn = resolve<PFN_vkGetPhysicalDeviceFeatures2>("vkGetPhysicalDeviceFeatures2");
    if (fn) fn(device, features);
}
extern "C" VKAPI_ATTR void VKAPI_CALL vkCmdCopyBuffer(VkCommandBuffer commandBuffer, VkBuffer srcBuffer, VkBuffer dstBuffer, uint32_t regionCount, const VkBufferCopy *regions) {
    static auto fn = resolve<PFN_vkCmdCopyBuffer>("vkCmdCopyBuffer");
    if (fn) fn(commandBuffer, srcBuffer, dstBuffer, regionCount, regions);
}
extern "C" VKAPI_ATTR VkResult VKAPI_CALL sb_vk_missing_stub() {
    return VK_ERROR_INITIALIZATION_FAILED;
}
extern "C" VKAPI_ATTR PFN_vkVoidFunction VKAPI_CALL sb_vkGetInstanceProcAddr(VkInstance instance, const char *name) {
    (void)name;
    auto proc = entry();
    // Must not throw or return null: Vulkan-Hpp's noexcept dispatcher init
    // calls the returned pointer, so hand back a failing stub instead.
    // Hpp turns VK_ERROR_INITIALIZATION_FAILED into a caught SystemError,
    // leaving the CPU registry intact.
    if (!proc) return reinterpret_cast<PFN_vkVoidFunction>(&sb_vk_missing_stub);
    return proc(instance, name);
}
