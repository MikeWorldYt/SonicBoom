cmake_minimum_required(VERSION 3.22)
project(sonicboom_qwen_llama LANGUAGES C CXX)
set(CMAKE_CXX_STANDARD 17)
set(CMAKE_POSITION_INDEPENDENT_CODE ON)
set(BUILD_SHARED_LIBS OFF CACHE BOOL "" FORCE)
set(LLAMA_BUILD_TESTS OFF CACHE BOOL "" FORCE)
set(LLAMA_BUILD_EXAMPLES OFF CACHE BOOL "" FORCE)
set(LLAMA_BUILD_TOOLS OFF CACHE BOOL "" FORCE)
set(LLAMA_BUILD_SERVER OFF CACHE BOOL "" FORCE)
set(LLAMA_BUILD_COMMON OFF CACHE BOOL "" FORCE)
set(GGML_NATIVE OFF CACHE BOOL "" FORCE)
set(GGML_OPENMP OFF CACHE BOOL "" FORCE)
set(GGML_METAL OFF CACHE BOOL "" FORCE)
set(GGML_ACCELERATE OFF CACHE BOOL "" FORCE)
set(GGML_BACKEND_DL OFF CACHE BOOL "" FORCE)
# CPU-only artifacts must run on baseline x86_64, not just the build host.
foreach(flag AVX AVX2 AVX512 FMA F16C BMI2 SSE42)
  set(GGML_${flag} OFF CACHE BOOL "" FORCE)
endforeach()
if(WIN32)
  # Keep the nested shader-generator build below MSBuild's path-length limit.
  set_property(DIRECTORY PROPERTY EP_BASE "${CMAKE_BINARY_DIR}/ep")
endif()
# Multi-GPU NCCL collectives are unnecessary for SonicBoom's single-service TTS
# server. The pinned llama.cpp defaults GGML_CUDA_NCCL=ON, so ggml-cuda.a
# references NCCL symbols whenever the builder image happens to ship libnccl;
# Cargo's final link line never passes -lnccl, producing
# "undefined reference to ncclCommDestroy" and friends. Default it OFF and
# expose an explicit opt-in. build.rs forwards SONICBOOM_CUDA_NCCL and links
# the resolved NCCL library when it is enabled. Ordinary single-GPU CUDA
# inference is unaffected either way.
option(SONICBOOM_CUDA_NCCL "Enable NCCL multi-GPU collectives in ggml-cuda" OFF)
set(GGML_CUDA_NCCL ${SONICBOOM_CUDA_NCCL} CACHE BOOL "" FORCE)
# CUDA architectures to compile ggml-cuda for. Empty keeps the pinned llama.cpp
# release default, which covers Maxwell through Blackwell and is required for
# release binaries. CI smoke builds may pass a narrow set (e.g. "90-virtual")
# via SONICBOOM_CUDA_ARCHITECTURES to cut NVCC time without changing what
# production ships.
set(SONICBOOM_CUDA_ARCHITECTURES "" CACHE STRING
  "Override CMAKE_CUDA_ARCHITECTURES for ggml-cuda (empty keeps the full default)")
if(GGML_CUDA AND SONICBOOM_CUDA_ARCHITECTURES)
  set(CMAKE_CUDA_ARCHITECTURES "${SONICBOOM_CUDA_ARCHITECTURES}" CACHE STRING "" FORCE)
endif()
if(GGML_CUDA)
  find_package(CUDAToolkit REQUIRED)
  # Cargo does not inherit transitive CMake link directories from static ggml.
  set(SONICBOOM_CUDA_LINK_FILES
    "$<TARGET_FILE:CUDA::cudart>\n$<TARGET_FILE:CUDA::cublas>\n$<TARGET_FILE:CUDA::cublasLt>\n$<TARGET_FILE:CUDA::cuda_driver>")
  if(GGML_CUDA_NCCL)
    find_package(NCCL REQUIRED)
    string(APPEND SONICBOOM_CUDA_LINK_FILES "\n$<TARGET_FILE:NCCL::NCCL>")
  endif()
  file(GENERATE OUTPUT "${CMAKE_BINARY_DIR}/cuda-link-paths.txt" CONTENT
    "${SONICBOOM_CUDA_LINK_FILES}\n")
endif()
if(GGML_VULKAN)
  # Find Vulkan in this scope: the vendored ggml-vulkan find_package() runs
  # in a subdirectory scope, so Vulkan_INCLUDE_DIRS would otherwise be empty
  # here (fatal on Windows, where MSVC has no default Vulkan include path).
  find_package(Vulkan REQUIRED)
endif()
add_subdirectory(../../vendor/llama.cpp llama)
# The pinned llama.cpp revision discovers the SPIRV-Headers CMake package but
# never links its imported target, so ggml-vulkan.cpp's `#include <spirv/...>`
# only resolves when the headers happen to sit in a compiler-default directory.
# build.rs locates them and passes SONICBOOM_SPIRV_INCLUDE_DIR so the Vulkan
# backend compiles from a non-standard (e.g. ~/.local) installation too.
if(TARGET ggml-vulkan)
  if(TARGET SPIRV-Headers::SPIRV-Headers)
    target_link_libraries(ggml-vulkan PRIVATE SPIRV-Headers::SPIRV-Headers)
  endif()
  if(SONICBOOM_SPIRV_INCLUDE_DIR)
    target_include_directories(ggml-vulkan PRIVATE "${SONICBOOM_SPIRV_INCLUDE_DIR}")
  endif()
  # The pinned backend uses Hpp dynamic dispatch with one direct entry point.
  # Redirect that entry point to our optional loader; never import Vulkan at startup.
  target_compile_definitions(ggml-vulkan PRIVATE vkGetInstanceProcAddr=sb_vkGetInstanceProcAddr)
  get_target_property(vk_links ggml-vulkan LINK_LIBRARIES)
  list(REMOVE_ITEM vk_links Vulkan::Vulkan)
  set_property(TARGET ggml-vulkan PROPERTY LINK_LIBRARIES "${vk_links}")
  target_include_directories(ggml-vulkan PRIVATE ${Vulkan_INCLUDE_DIRS})
endif()
add_library(sonicboom-qwen-llama STATIC shim.cpp)
target_link_libraries(sonicboom-qwen-llama PRIVATE llama)
if(GGML_VULKAN)
  target_sources(sonicboom-qwen-llama PRIVATE vulkan-loader.cpp)
  target_include_directories(sonicboom-qwen-llama PRIVATE ${Vulkan_INCLUDE_DIRS})
  target_compile_definitions(sonicboom-qwen-llama PRIVATE SONICBOOM_VULKAN)
endif()
install(TARGETS sonicboom-qwen-llama ARCHIVE DESTINATION lib)
