use super::c;

#[repr(transparent)]
pub struct BlipMutRef(pub(super) *mut c::blip_t);

impl BlipMutRef {
    pub fn set_rates(&mut self, clock_rate: f64, sample_rate: f64) {
        unsafe { c::blip_set_rates(self.0, clock_rate, sample_rate) }
    }

    pub fn samples_avail(&self) -> i32 {
        unsafe { c::blip_samples_avail(self.0) }
    }

    pub fn read_samples(&self, out: &mut [i16], count: i32, stereo: bool) -> i32 {
        unsafe { c::blip_read_samples(self.0, out.as_mut_ptr(), count, if stereo { 1 } else { 0 }) }
    }
}
