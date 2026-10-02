use tract_onnx::prelude::*;

/// Loads the ONNX bytes, fixes the input to 1 x 4 x h x w, runs `x` (4*h*w floats) and writes the score map
/// (h/4 * w/4 floats) to `score`. Returns 0 on success.
#[unsafe(no_mangle)]
pub extern "C" fn run_model(
    model: *const u8,
    model_len: usize,
    x: *const f32,
    h: usize,
    w: usize,
    score: *mut f32,
) -> i32 {
    let go = || -> TractResult<()> {
        let bytes = unsafe { std::slice::from_raw_parts(model, model_len) };
        let plan = tract_onnx::onnx()
            .with_ignore_value_info(true)
            .model_for_read(&mut &bytes[..])?
            .with_input_fact(0, f32::fact([1, 4, h, w]).into())?
            .into_optimized()?
            .into_runnable()?;
        let x = unsafe { std::slice::from_raw_parts(x, 4 * h * w) };
        let out = plan.run(tvec!(Tensor::from_shape(&[1, 4, h, w], x)?.into()))?;
        let s = out[0].try_as_plain_ram()?;
        let s = s.as_slice::<f32>()?;
        unsafe { std::ptr::copy_nonoverlapping(s.as_ptr(), score, s.len()) };
        Ok(())
    };
    match go() {
        Ok(()) => 0,
        Err(_) => 1,
    }
}
