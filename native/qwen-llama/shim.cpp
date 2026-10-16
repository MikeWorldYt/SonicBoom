// SonicBoom-owned, narrow C ABI. All llama.cpp structs stay on the C++ side.
// Compiled only against vendor/llama.cpp c1d0e7a004015f23bc0233470b747b596f29b264.
#include "llama.h"
#include "ggml-backend.h"
#include <algorithm>
#include <cmath>
#include <cstring>
#include <memory>
#include <mutex>
#include <stdexcept>
#include <string>
#include <vector>

namespace {
thread_local std::string error;
std::mutex native_execution;
struct State {
    llama_model * model = nullptr;
    llama_context * ctx = nullptr;
    int dim = 0, vocab = 0, ctx_size = 0;
    bool talker = false;
    ~State() { if (ctx) llama_free(ctx); if (model) llama_model_free(model); }
};
void initialize() {
    static std::once_flag once;
    std::call_once(once, [] { ggml_backend_load_all(); llama_backend_init(); });
}
}
extern "C" {
const char * sb_qwen_error() noexcept { return error.c_str(); }
// device: 0 CPU, 1 Vulkan, 2 CUDA. No implicit fallback inside a model handle.
void * sb_qwen_open(const char * path, int device, int gpu_layers, int threads, int context, int talker) noexcept {
    try {
        std::lock_guard<std::mutex> guard(native_execution);
        error.clear();
        initialize();
        auto state = std::make_unique<State>();
        auto mp = llama_model_default_params();
        ggml_backend_dev_t devices[2] = {nullptr, nullptr};
        if (device != 0) {
            const char * wanted = device == 1 ? "Vulkan" : device == 2 ? "CUDA" : "invalid";
            for (size_t i = 0; i < ggml_backend_dev_count(); ++i) {
                auto dev = ggml_backend_dev_get(i);
                const char * reg = ggml_backend_reg_name(ggml_backend_dev_backend_reg(dev));
                if (std::string(reg) == wanted) { devices[0] = dev; break; }
            }
            if (!devices[0]) throw std::runtime_error(std::string(wanted) + " device unavailable in this build/driver");
            if (gpu_layers == 0) throw std::runtime_error("GPU requested with zero offload layers");
        }
        mp.devices = devices;
        mp.n_gpu_layers = device == 0 ? 0 : gpu_layers < 0 ? 999 : gpu_layers;
        mp.check_tensors = true;
        state->model = llama_model_load_from_file(path, mp);
        if (!state->model) throw std::runtime_error("llama.cpp model load failed");
        state->dim = llama_model_n_embd_inp(state->model);
        state->vocab = llama_vocab_n_tokens(llama_model_get_vocab(state->model));
        state->talker = talker != 0;
        if (state->dim != (talker ? 2048 : 1024) || state->vocab != (talker ? 3072 : 30720))
            throw std::runtime_error("incompatible GGUF embedding/vocabulary dimensions");
        auto cp = llama_context_default_params();
        cp.n_ctx = context;
        cp.n_batch = 512;
        cp.n_ubatch = 512;
        cp.n_threads = threads;
        cp.n_threads_batch = threads;
        cp.embeddings = talker != 0;
        cp.pooling_type = LLAMA_POOLING_TYPE_NONE;
        cp.flash_attn_type = LLAMA_FLASH_ATTN_TYPE_DISABLED;
        cp.offload_kqv = device != 0;
        cp.op_offload = device != 0;
        state->ctx = llama_init_from_model(state->model, cp);
        if (!state->ctx) throw std::runtime_error("llama.cpp context initialization failed");
        state->ctx_size = static_cast<int>(llama_n_ctx(state->ctx));
        return state.release();
    } catch (const std::exception & e) { error = e.what(); return nullptr; }
      catch (...) { error = "unknown native model initialization failure"; return nullptr; }
}
void sb_qwen_close(void * ptr) noexcept { std::lock_guard<std::mutex> guard(native_execution); delete static_cast<State *>(ptr); }
int sb_qwen_clear(void * ptr) noexcept {
    try { std::lock_guard<std::mutex> guard(native_execution); if (!ptr) return -1; llama_memory_clear(llama_get_memory(static_cast<State *>(ptr)->ctx), true); return 0; }
    catch (...) { error = "native KV clear failed"; return -1; }
}
int sb_qwen_decode(void * ptr, const float * embeddings, int count, int position,
                   float * logits, size_t logits_len, float * hidden, size_t hidden_len) noexcept {
    try {
        std::lock_guard<std::mutex> guard(native_execution);
        if (!ptr || !embeddings || !logits) throw std::runtime_error("null native decode pointer");
        auto & s = *static_cast<State *>(ptr);
        if (count < 1 || count > 512 || position < 0 || position > s.ctx_size - count || logits_len != static_cast<size_t>(s.vocab))
            throw std::runtime_error("native decode dimensions/context invalid");
        if (s.talker && (!hidden || hidden_len != static_cast<size_t>(s.dim)))
            throw std::runtime_error("native hidden output dimensions invalid");
        const size_t size = static_cast<size_t>(count) * s.dim;
        if (!std::all_of(embeddings, embeddings + size, [](float x) { return std::isfinite(x); }))
            throw std::runtime_error("non-finite native input embedding");
        // Construct a borrowed batch: all storage survives synchronous llama_decode.
        std::vector<llama_pos> positions(count * (s.talker ? 4 : 1), 0);
        std::vector<int32_t> seq_counts(count, 1);
        std::vector<llama_seq_id> seqs(count, 0);
        std::vector<llama_seq_id *> seq_ptrs(count);
        std::vector<int8_t> output(count, 0);
        output.back() = 1;
        for (int i = 0; i < count; ++i) {
            seq_ptrs[i] = &seqs[i];
            for (int plane = 0; plane < (s.talker ? 3 : 1); ++plane)
                positions[plane * count + i] = position + i;
        }
        llama_batch batch{};
        batch.n_tokens = count;
        batch.embd = const_cast<float *>(embeddings);
        batch.pos = positions.data();
        batch.n_seq_id = seq_counts.data();
        batch.seq_id = seq_ptrs.data();
        batch.logits = output.data();
        const int result = llama_decode(s.ctx, batch);
        if (result != 0) throw std::runtime_error("llama_decode failed: " + std::to_string(result));
        auto lp = llama_get_logits_ith(s.ctx, -1);
        if (!lp) throw std::runtime_error("llama.cpp returned no logits");
        std::copy(lp, lp + s.vocab, logits);
        if (s.talker) {
            auto hp = llama_get_embeddings_ith(s.ctx, -1);
            if (!hp) throw std::runtime_error("llama.cpp returned no hidden state");
            std::copy(hp, hp + s.dim, hidden);
        }
        return 0;
    } catch (const std::exception & e) { error = e.what(); return -1; }
      catch (...) { error = "unknown native decode failure"; return -1; }
}
}
