use super::c;

#[repr(transparent)]
pub struct BlipMutRef<'a> {
    pub(super) ptr: *mut c::blip_t,
    pub(super) _marker: std::marker::PhantomData<&'a ()>,
}

impl<'a> BlipMutRef<'a> {
    pub fn set_rates(&mut self, clock_rate: f64, sample_rate: f64) {
        unsafe { c::blip_set_rates(self.ptr, clock_rate, sample_rate) }
    }

    pub fn samples_avail(&self) -> i32 {
        unsafe { c::blip_samples_avail(self.ptr) }
    }

    pub fn read_samples(&self, out: &mut [i16], count: i32, stereo: bool) -> i32 {
        unsafe {
            c::blip_read_samples(
                self.ptr,
                out.as_mut_ptr(),
                count,
                if stereo { 1 } else { 0 },
            )
        }
    }
}
