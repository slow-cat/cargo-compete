//! Runtime accounting for generated input size.

pub(super) struct Budget {
    pub(super) used: u128,
    limit: u128,
}

impl Budget {
    pub(super) fn new(limit: u128) -> Self {
        Self { used: 0, limit }
    }

    pub(super) fn add(&mut self, n: u128) -> Result<(), String> {
        self.used = self.used.checked_add(n).ok_or_else(|| {
            format!(
                "input too large: generated case element count overflows 128-bit range \
                 exceeds the safety ceiling {}",
                self.limit
            )
        })?;
        if self.used > self.limit {
            return Err(format!(
                "input too large: generated case has at least {} elements exceeds the safety ceiling {}; \
                 narrow the range in the yml `vars:` section",
                self.used, self.limit
            ));
        }
        Ok(())
    }
}
