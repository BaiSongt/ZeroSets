use glam::Vec3;

/// Axis-aligned bounding box.
///
/// `Aabb::EMPTY` uses inverted infinities, so `union`/`intersection` behave as
/// the identity/absorbing elements without a separate `Option`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Aabb {
    pub min: Vec3,
    pub max: Vec3,
}

impl Aabb {
    pub const EMPTY: Self = Self {
        min: Vec3::splat(f32::INFINITY),
        max: Vec3::splat(f32::NEG_INFINITY),
    };

    #[inline]
    pub fn new(min: Vec3, max: Vec3) -> Self {
        Self {
            min: min.min(max),
            max: min.max(max),
        }
    }

    #[inline]
    pub fn from_center_half(center: Vec3, half: Vec3) -> Self {
        let half = half.abs();
        Self {
            min: center - half,
            max: center + half,
        }
    }

    #[inline]
    pub fn is_empty(&self) -> bool {
        self.min.x > self.max.x || self.min.y > self.max.y || self.min.z > self.max.z
    }

    #[inline]
    pub fn is_finite(&self) -> bool {
        self.min.is_finite() && self.max.is_finite()
    }

    #[inline]
    pub fn union(self, other: Self) -> Self {
        if self.is_empty() {
            return other;
        }
        if other.is_empty() {
            return self;
        }
        Self {
            min: self.min.min(other.min),
            max: self.max.max(other.max),
        }
    }

    #[inline]
    pub fn intersection(self, other: Self) -> Self {
        if self.is_empty() || other.is_empty() {
            return Self::EMPTY;
        }
        let min = self.min.max(other.min);
        let max = self.max.min(other.max);
        if min.x > max.x || min.y > max.y || min.z > max.z {
            Self::EMPTY
        } else {
            Self { min, max }
        }
    }

    #[inline]
    pub fn expanded(self, distance: f32) -> Self {
        if self.is_empty() || !distance.is_finite() {
            return self;
        }
        let min = self.min - Vec3::splat(distance);
        let max = self.max + Vec3::splat(distance);
        if min.x > max.x || min.y > max.y || min.z > max.z {
            Self::EMPTY
        } else {
            Self { min, max }
        }
    }

    #[inline]
    pub fn size(self) -> Vec3 {
        if self.is_empty() {
            Vec3::ZERO
        } else {
            self.max - self.min
        }
    }

    #[inline]
    pub fn half(self) -> Vec3 {
        self.size() * 0.5
    }

    #[inline]
    pub fn center(self) -> Vec3 {
        if self.is_empty() {
            Vec3::ZERO
        } else {
            (self.min + self.max) * 0.5
        }
    }

    #[inline]
    pub fn max_extent(self) -> f32 {
        self.size().max_element()
    }

    #[inline]
    pub fn contains(self, point: Vec3) -> bool {
        !self.is_empty()
            && point.x >= self.min.x
            && point.x <= self.max.x
            && point.y >= self.min.y
            && point.y <= self.max.y
            && point.z >= self.min.z
            && point.z <= self.max.z
    }

    /// The eight corners, in `(x, y, z)` bit order.
    #[inline]
    pub fn corners(self) -> [Vec3; 8] {
        [
            Vec3::new(self.min.x, self.min.y, self.min.z),
            Vec3::new(self.max.x, self.min.y, self.min.z),
            Vec3::new(self.min.x, self.max.y, self.min.z),
            Vec3::new(self.max.x, self.max.y, self.min.z),
            Vec3::new(self.min.x, self.min.y, self.max.z),
            Vec3::new(self.max.x, self.min.y, self.max.z),
            Vec3::new(self.min.x, self.max.y, self.max.z),
            Vec3::new(self.max.x, self.max.y, self.max.z),
        ]
    }
}

impl Default for Aabb {
    fn default() -> Self {
        Self::EMPTY
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_is_identity_for_union() {
        let a = Aabb::from_center_half(Vec3::ZERO, Vec3::ONE);
        assert_eq!(Aabb::EMPTY.union(a), a);
        assert_eq!(a.union(Aabb::EMPTY), a);
        assert!(Aabb::EMPTY.intersection(a).is_empty());
    }

    #[test]
    fn intersection_can_be_empty() {
        let a = Aabb::from_center_half(Vec3::ZERO, Vec3::ONE);
        let b = Aabb::from_center_half(Vec3::splat(4.0), Vec3::ONE);
        assert!(a.intersection(b).is_empty());
    }
}
