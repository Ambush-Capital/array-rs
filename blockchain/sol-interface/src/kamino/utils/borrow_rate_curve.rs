use borsh::{BorshDeserialize, BorshSerialize};
use bytemuck::Zeroable;

use crate::kamino::utils::consts::FULL_BPS;
use crate::kamino::utils::errors::LendingError;
use crate::kamino::utils::fraction::{Fraction, FractionExtra};

pub const MAX_UTILIZATION_RATE_BPS: u32 = FULL_BPS as u32;

#[derive(BorshDeserialize, BorshSerialize, Debug, PartialEq, Eq, Zeroable, Clone)]
#[repr(C)]
pub struct BorrowRateCurve {
    pub points: [CurvePoint; 11],
}

impl<'de> serde::Deserialize<'de> for BorrowRateCurve {
    fn deserialize<D>(deserializer: D) -> std::result::Result<BorrowRateCurve, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let points = <Vec<CurvePoint> as serde::Deserialize>::deserialize(deserializer)?;
        BorrowRateCurve::from_points(&points).map_err(serde::de::Error::custom)
    }
}

impl serde::Serialize for BorrowRateCurve {
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        let mut end_reached = false;
        let points = self
            .points
            .iter()
            .take_while(|p| {
                if end_reached {
                    return false;
                } else if p.utilization_rate_bps == MAX_UTILIZATION_RATE_BPS {
                    end_reached = true;
                }
                true
            })
            .collect::<Vec<_>>();
        serde::Serialize::serialize(&points, serializer)
    }
}

impl Default for BorrowRateCurve {
    fn default() -> Self {
        BorrowRateCurve::new_flat(0)
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CurveSegment {
    pub slope_nom: u32,
    pub slope_denom: u32,
    pub start_point: CurvePoint,
}

#[derive(
    BorshSerialize,
    BorshDeserialize,
    Debug,
    Default,
    PartialEq,
    Eq,
    Zeroable,
    Clone,
    Copy,
    serde::Deserialize,
    serde::Serialize,
)]
#[serde(deny_unknown_fields)]
#[repr(C)]
pub struct CurvePoint {
    pub utilization_rate_bps: u32,
    pub borrow_rate_bps: u32,
}

impl CurvePoint {
    pub fn new(utilization_rate_bps: u32, borrow_rate_bps: u32) -> Self {
        Self { utilization_rate_bps, borrow_rate_bps }
    }
}

impl CurveSegment {
    pub fn from_points(start: CurvePoint, end: CurvePoint) -> Result<Self, LendingError> {
        let slope_nom = end
            .borrow_rate_bps
            .checked_sub(start.borrow_rate_bps)
            .ok_or(LendingError::InvalidBorrowRateCurvePoint)?;
        if end.utilization_rate_bps <= start.utilization_rate_bps {
            return Err(LendingError::InvalidBorrowRateCurvePoint);
        }
        let slope_denom = end.utilization_rate_bps.checked_sub(start.utilization_rate_bps).unwrap();

        Ok(CurveSegment { slope_nom, slope_denom, start_point: start })
    }

    pub(self) fn get_borrow_rate(
        &self,
        utilization_rate: Fraction,
    ) -> Result<Fraction, LendingError> {
        let start_utilization_rate = Fraction::from_bps(self.start_point.utilization_rate_bps);

        let coef = utilization_rate
            .checked_sub(start_utilization_rate)
            .ok_or(LendingError::InvalidUtilizationRate)?;

        let nom = coef * u128::from(self.slope_nom);
        let base_rate = nom / u128::from(self.slope_denom);

        let offset = Fraction::from_bps(self.start_point.borrow_rate_bps);

        Ok(base_rate + offset)
    }
}

impl BorrowRateCurve {
    pub fn validate(&self) -> Result<(), LendingError> {
        let pts = &self.points;

        if pts[0].utilization_rate_bps != 0 {
            return Err(LendingError::InvalidBorrowRateCurvePoint);
        }

        if pts[10].utilization_rate_bps != MAX_UTILIZATION_RATE_BPS {
            return Err(LendingError::InvalidBorrowRateCurvePoint);
        }

        let mut last_pt = pts[0];
        for pt in pts.iter().skip(1) {
            if last_pt.utilization_rate_bps == MAX_UTILIZATION_RATE_BPS {
                if pt.utilization_rate_bps != MAX_UTILIZATION_RATE_BPS {
                    return Err(LendingError::InvalidBorrowRateCurvePoint);
                }
            } else if pt.utilization_rate_bps <= last_pt.utilization_rate_bps {
                return Err(LendingError::InvalidBorrowRateCurvePoint);
            }
            if pt.borrow_rate_bps < last_pt.borrow_rate_bps {
                return Err(LendingError::InvalidBorrowRateCurvePoint);
            }
            last_pt = *pt;
        }
        Ok(())
    }

    pub fn from_points(pts: &[CurvePoint]) -> Result<Self, LendingError> {
        if pts.len() < 2 {
            return Err(LendingError::InvalidBorrowRateCurvePoint);
        }
        if pts.len() > 11 {
            return Err(LendingError::InvalidBorrowRateCurvePoint);
        }
        let last = pts.last().unwrap();
        if last.utilization_rate_bps != MAX_UTILIZATION_RATE_BPS {
            return Err(LendingError::InvalidBorrowRateCurvePoint);
        }
        let mut points = [*last; 11];

        points[..pts.len()].copy_from_slice(pts);

        let curve = BorrowRateCurve { points };
        curve.validate()?;
        Ok(curve)
    }

    pub fn new_flat(borrow_rate_bps: u32) -> Self {
        let points = [
            CurvePoint { utilization_rate_bps: 0, borrow_rate_bps },
            CurvePoint { utilization_rate_bps: MAX_UTILIZATION_RATE_BPS, borrow_rate_bps },
        ];
        Self::from_points(&points).unwrap()
    }

    pub fn from_legacy_parameters(
        optimal_utilization_rate_pct: u8,
        base_rate_pct: u8,
        optimal_rate_pct: u8,
        max_rate_pct: u8,
    ) -> Self {
        let optimal_utilization_rate = u32::from(optimal_utilization_rate_pct) * 100;
        let base_rate = u32::from(base_rate_pct) * 100;
        let optimal_rate = u32::from(optimal_rate_pct) * 100;
        let max_rate = u32::from(max_rate_pct) * 100;
        let alloc_1;
        let alloc_2;

        let points: &[CurvePoint] = if optimal_utilization_rate == 0 {
            alloc_1 = [
                CurvePoint { utilization_rate_bps: 0, borrow_rate_bps: optimal_rate },
                CurvePoint {
                    utilization_rate_bps: MAX_UTILIZATION_RATE_BPS,
                    borrow_rate_bps: max_rate,
                },
            ];
            &alloc_1
        } else if optimal_utilization_rate == MAX_UTILIZATION_RATE_BPS {
            alloc_1 = [
                CurvePoint { utilization_rate_bps: 0, borrow_rate_bps: base_rate },
                CurvePoint {
                    utilization_rate_bps: MAX_UTILIZATION_RATE_BPS,
                    borrow_rate_bps: optimal_rate,
                },
            ];
            &alloc_1
        } else {
            alloc_2 = [
                CurvePoint { utilization_rate_bps: 0, borrow_rate_bps: base_rate },
                CurvePoint {
                    utilization_rate_bps: optimal_utilization_rate,
                    borrow_rate_bps: optimal_rate,
                },
                CurvePoint {
                    utilization_rate_bps: MAX_UTILIZATION_RATE_BPS,
                    borrow_rate_bps: max_rate,
                },
            ];
            &alloc_2
        };
        Self::from_points(points).unwrap()
    }

    pub fn get_borrow_rate(&self, utilization_rate: Fraction) -> Result<Fraction, LendingError> {
        let utilization_rate =
            if utilization_rate > Fraction::ONE { Fraction::ONE } else { utilization_rate };

        let utilization_rate_bps: u32 = utilization_rate.to_bps().unwrap();

        let (start_pt, end_pt) = self
            .points
            .windows(2)
            .map(|seg| {
                let [first, second]: &[CurvePoint; 2] = seg.try_into().unwrap();
                (first, second)
            })
            .find(|(first, second)| {
                utilization_rate_bps >= first.utilization_rate_bps
                    && utilization_rate_bps <= second.utilization_rate_bps
            })
            .unwrap();
        if utilization_rate_bps == start_pt.utilization_rate_bps {
            return Ok(Fraction::from_bps(start_pt.borrow_rate_bps));
        } else if utilization_rate_bps == end_pt.utilization_rate_bps {
            return Ok(Fraction::from_bps(end_pt.borrow_rate_bps));
        }

        let segment = CurveSegment::from_points(*start_pt, *end_pt)?;

        segment.get_borrow_rate(utilization_rate)
    }
}
