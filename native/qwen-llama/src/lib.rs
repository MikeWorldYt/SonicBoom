//! Narrow, owned llama.cpp FFI. Handles are thread-confined (`!Send + !Sync`).
use std::{
    ffi::{CStr, CString, c_char, c_void},
    marker::PhantomData,
    ptr::NonNull,
    rc::Rc,
};

unsafe extern "C" {
    fn sb_qwen_backends() -> *const c_char;
    fn sb_qwen_error() -> *const c_char;
    fn sb_qwen_open(
        path: *const c_char,
        device: i32,
        gpu_layers: i32,
        threads: i32,
        context: i32,
        talker: i32,
    ) -> *mut c_void;
    fn sb_qwen_close(ptr: *mut c_void);
    fn sb_qwen_clear(ptr: *mut c_void) -> i32;
    fn sb_qwen_decode(
        ptr: *mut c_void,
        input: *const f32,
        count: i32,
        position: i32,
        logits: *mut f32,
        logits_len: usize,
        hidden: *mut f32,
        hidden_len: usize,
    ) -> i32;
}
fn error() -> String {
    // SAFETY: shim returns a thread-local nul-terminated string valid until next call.
    unsafe {
        CStr::from_ptr(sb_qwen_error())
            .to_string_lossy()
            .into_owned()
    }
}

pub struct Model {
    handle: NonNull<c_void>,
    dim: usize,
    vocab: usize,
    context: usize,
    _thread: PhantomData<Rc<()>>,
}

impl Model {
    pub fn load(
        path: &str,
        device: i32,
        gpu_layers: i32,
        threads: usize,
        context: usize,
        talker: bool,
    ) -> Result<Self, String> {
        if threads == 0
            || threads > 256
            || !(32..=8192).contains(&context)
            || !(0..=2).contains(&device)
        {
            return Err("invalid native model options".into());
        }
        let path = CString::new(path).map_err(|_| "model path contains NUL")?;
        // SAFETY: path remains live; all options validated; shim owns all native structs.
        let handle = unsafe {
            sb_qwen_open(
                path.as_ptr(),
                device,
                gpu_layers,
                threads as i32,
                context as i32,
                talker.into(),
            )
        };
        Ok(Self {
            handle: NonNull::new(handle).ok_or_else(error)?,
            dim: if talker { 2048 } else { 1024 },
            vocab: if talker { 3072 } else { 30720 },
            context,
            _thread: PhantomData,
        })
    }
    pub fn clear(&mut self) -> Result<(), String> {
        // SAFETY: live exclusive handle on its owner thread.
        if unsafe { sb_qwen_clear(self.handle.as_ptr()) } == 0 {
            Ok(())
        } else {
            Err(error())
        }
    }
    pub fn decode(
        &mut self,
        embeddings: &[f32],
        position: usize,
    ) -> Result<(Vec<f32>, Vec<f32>), String> {
        let count = embeddings.len() / self.dim;
        if !embeddings.len().is_multiple_of(self.dim)
            || !(1..=512).contains(&count)
            || position
                .checked_add(count)
                .is_none_or(|end| end > self.context)
        {
            return Err("invalid native embedding shape/context".into());
        }
        let mut logits = vec![0.0; self.vocab];
        let mut hidden = vec![0.0; if self.dim == 2048 { self.dim } else { 0 }];
        // SAFETY: input and output buffers have exactly the sizes expected by the shim,
        // remain live for the synchronous call, and exclusive ownership prevents races.
        let result = unsafe {
            sb_qwen_decode(
                self.handle.as_ptr(),
                embeddings.as_ptr(),
                count as i32,
                position as i32,
                logits.as_mut_ptr(),
                logits.len(),
                hidden.as_mut_ptr(),
                hidden.len(),
            )
        };
        if result != 0 {
            return Err(error());
        }
        if logits.iter().chain(&hidden).any(|v| !v.is_finite()) {
            return Err("non-finite native model output".into());
        }
        Ok((logits, hidden))
    }
}
impl Drop for Model {
    fn drop(&mut self) {
        // SAFETY: unique handle, released once; no outstanding native calls or borrows.
        unsafe {
            sb_qwen_close(self.handle.as_ptr());
        }
    }
}

/// Model-free discovery; contains device descriptions, never local paths.
pub fn runtime_backends() -> String {
    // SAFETY: thread-local string copied before the next native call.
    unsafe {
        CStr::from_ptr(sb_qwen_backends())
            .to_string_lossy()
            .into_owned()
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn cpu_registered_in_every_build() {
        assert!(super::runtime_backends().lines().any(|s| s == "cpu"));
    }
}
