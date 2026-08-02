//! Min/max de temperatura por pneu durante a sessão (reset ao trocar de carro).

const MAX_VALID_TEMP: f32 = 900.0;

#[derive(Clone, Copy, Debug, Default)]
pub struct TireRange {
    pub min: [f32; 4],
    pub max: [f32; 4],
    /// Pneu já recebeu leitura válida nesta sessão.
    pub seen: [bool; 4],
}

impl TireRange {
    pub fn any(&self) -> bool {
        self.seen.iter().any(|&s| s)
    }
}

pub struct TireTracker {
    range: TireRange,
}

impl TireTracker {
    pub fn new() -> Self {
        Self {
            range: TireRange::default(),
        }
    }

    pub fn reset(&mut self) {
        *self = Self::new();
    }

    pub fn update(&mut self, tires: Option<[f32; 4]>, active: bool) {
        if !active {
            return;
        }
        let Some(temps) = tires else {
            return;
        };
        for (i, &v) in temps.iter().enumerate() {
            if !v.is_finite() || v <= 0.0 || v > MAX_VALID_TEMP {
                continue;
            }
            if !self.range.seen[i] {
                self.range.min[i] = v;
                self.range.max[i] = v;
                self.range.seen[i] = true;
            } else {
                self.range.min[i] = self.range.min[i].min(v);
                self.range.max[i] = self.range.max[i].max(v);
            }
        }
    }

    pub fn range(&self) -> &TireRange {
        &self.range
    }
}

impl Default for TireTracker {
    fn default() -> Self {
        Self::new()
    }
}

/// Cor por faixa de temperatura (mesma lógica do apex-flow).
pub fn temp_color(temp: f32) -> (f32, f32, f32, f32) {
    if temp < 150.0 {
        (0.227, 0.639, 1.0, 0.92)
    } else if temp < 205.0 {
        (0.224, 0.827, 0.325, 0.92)
    } else if temp < 245.0 {
        (0.890, 0.702, 0.255, 0.94)
    } else {
        // RGBA channels, not a mathematical constant: 0.318 is a red tone that
        // happens to sit near 1/pi.
        #[allow(clippy::approx_constant)]
        (0.973, 0.318, 0.286, 0.96)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn acumula_min_max_por_roda() {
        let mut tr = TireTracker::new();
        tr.update(Some([100.0, 120.0, 90.0, 110.0]), true);
        tr.update(Some([105.0, 115.0, 95.0, 130.0]), true);
        let r = tr.range();
        assert!(r.seen[0]);
        assert_eq!(r.min[0], 100.0);
        assert_eq!(r.max[0], 105.0);
        assert_eq!(r.max[3], 130.0);
    }
}
