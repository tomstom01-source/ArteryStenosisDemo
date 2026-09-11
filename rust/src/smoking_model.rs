//! Smoking-plaque model over an assumed 10-year timeline, grounded in the
//! CARDIA and PDAY studies.
//!
//! - **CARDIA** (primary; Pletcher et al., Arch Intern Med 2006) supplies the
//!   dose-response slope: among 1,535 CARDIA smokers followed since 1985 with
//!   coronary calcification measured at Year 15, the adjusted odds of coronary
//!   calcification rose ~1.27× per 10 pack-years of cumulative exposure.
//! - **PDAY** (supplementary; Strong et al., JAMA 1990 / ATVB 1999) measured
//!   atherosclerosis *directly at autopsy* (fatty streaks + raised lesions as
//!   percent intimal surface) in 15-34-year-olds, with smoking confirmed by
//!   postmortem **serum thiocyanate** - an objective biochemical marker rather
//!   than a questionnaire. PDAY shows smoking accelerates *early, largely
//!   non-calcified* plaque, which CARDIA's calcium-score endpoint cannot see;
//!   its contribution here is a documented correction factor for that
//!   non-calcified burden.
//! - **Quitting** is modelled the way CARDIA observed it prospectively:
//!   exposure accumulates only during the years actually smoked inside the
//!   10-year window. Plaque already formed does not regress (corroborated by
//!   calcification data showing former smokers' scores stay elevated for
//!   decades after quitting), so quitting stops further smoker-rate
//!   accumulation but does not remove what exists.

/// CARDIA: adjusted odds ratio for coronary calcification per 10 pack-years
/// (Pletcher et al., Arch Intern Med 2006; 1.27, 95% CI 1.01-1.60 menthol /
/// 1.33, 1.06-1.68 non-menthol - the pooled ~1.3 slope is applied per 10
/// pack-years).
pub const CARDIA_ODDS_RATIO_PER_10_PACK_YEARS: f64 = 1.27;

/// The assumed exposure/accumulation window of this simulation, in years.
pub const SMOKING_WINDOW_YEARS: f64 = 10.0;

/// PDAY correction: smoking's effect on *total* early plaque (fatty streaks +
/// raised lesions, measured objectively at autopsy) exceeds its effect on
/// calcified plaque alone, which is all a calcium score can see. Applied as a
/// multiplicative correction to the CARDIA-derived excess.
pub const PDAY_NONCALCIFIED_CORRECTION: f64 = 1.5;

/// Baseline 10-year plaque accumulation of the reference (least-healthy)
/// group, in percent - the same MESA-derived calibration base the lifestyle
/// slider uses, so all plaque sources share one scale.
pub const BASELINE_TEN_YEAR_ACCUMULATION_PERCENT: f64 = 45.0;

/// Years of the 10-year window actually spent smoking, set directly by the
/// "years of smoking" slider. A never-smoker (`cigarettes_per_day <= 0`)
/// accumulates nothing regardless of this value.
pub fn years_smoked_in_window(cigarettes_per_day: f64, years_of_smoking: f64) -> f64 {
    if cigarettes_per_day <= 0.0 {
        return 0.0;
    }
    years_of_smoking.clamp(0.0, SMOKING_WINDOW_YEARS)
}

/// Cumulative exposure in pack-years (1 pack-year = 20 cigarettes/day for one
/// year) over the years actually smoked inside the window.
pub fn pack_years(cigarettes_per_day: f64, years_smoked: f64) -> f64 {
    (cigarettes_per_day / 20.0) * years_smoked
}

/// Modelled 10-year plaque contribution from smoking, in percent (0..100).
///
/// `contribution = baseline × (1.27^(pack-years/10) − 1) × PDAY correction`,
/// where pack-years counts only the years of the window spent smoking.
/// Grounded in CARDIA's dose-response slope; scaled by PDAY's objective
/// autopsy evidence that smoking also drives the non-calcified early plaque a
/// calcium score cannot measure.
pub fn smoking_plaque_percent(cigarettes_per_day: f64, years_of_smoking: f64) -> f64 {
    if cigarettes_per_day <= 0.0 {
        return 0.0;
    }
    let years_smoked = years_smoked_in_window(cigarettes_per_day, years_of_smoking);
    let py = pack_years(cigarettes_per_day, years_smoked);
    let multiplier = CARDIA_ODDS_RATIO_PER_10_PACK_YEARS.powf(py / 10.0);
    (BASELINE_TEN_YEAR_ACCUMULATION_PERCENT * (multiplier - 1.0) * PDAY_NONCALCIFIED_CORRECTION)
        .clamp(0.0, 100.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn never_smoker_contributes_nothing() {
        for years in [0.0, 3.0, 7.0, 10.0] {
            let p = smoking_plaque_percent(0.0, years);
            assert_eq!(p, 0.0, "never-smoker contribution was {p}");
        }
    }

    #[test]
    fn dose_response_increases_with_cigarettes_per_day() {
        let mut prev = 0.0;
        for c in [5.0, 10.0, 20.0, 30.0, 40.0] {
            let p = smoking_plaque_percent(c, 10.0);
            assert!(p > prev, "contribution not increasing at {c} cig/day");
            prev = p;
        }
    }

    #[test]
    fn fewer_smoking_years_reduce_contribution_monotonically() {
        let mut prev = smoking_plaque_percent(20.0, 10.0);
        for years in [8.0, 5.0, 3.0, 1.0, 0.0] {
            let p = smoking_plaque_percent(20.0, years);
            assert!(p < prev, "{years}y of smoking did not reduce contribution");
            prev = p;
        }
        // Zero years of smoking in the window = never-smoker-equivalent.
        let p = smoking_plaque_percent(20.0, 0.0);
        assert!(p.abs() < 1e-9, "0y of smoking should contribute nothing");
    }

    #[test]
    fn contribution_is_bounded_and_matches_cardia_slope() {
        // 20 cigarettes/day for the full window = 10 pack-years -> CARDIA
        // multiplier 1.27 -> excess = 45% x 0.27 x 1.5 (PDAY) = ~18.2%.
        let p = smoking_plaque_percent(20.0, 10.0);
        let expected = BASELINE_TEN_YEAR_ACCUMULATION_PERCENT
            * (CARDIA_ODDS_RATIO_PER_10_PACK_YEARS - 1.0)
            * PDAY_NONCALCIFIED_CORRECTION;
        assert!((p - expected).abs() < 1e-9, "got {p}, expected {expected}");

        // 40/day (2 packs) for the full window = 20 pack-years.
        let max = smoking_plaque_percent(40.0, 10.0);
        assert!(max > p && max <= 100.0);
        // Never negative for any slider position.
        for c in [0.0, 10.0, 40.0] {
            for years in [0.0, 4.0, 10.0, 12.0] {
                assert!(smoking_plaque_percent(c, years) >= 0.0);
            }
        }
    }
}
