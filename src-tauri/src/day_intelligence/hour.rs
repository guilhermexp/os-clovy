//! The hour report: one call per completed hour with activity, answering a
//! short summary and the hour's activities with estimated minutes. The
//! minutes shown are Clovy's: the hour's measured active minutes split by the
//! model's estimates used only as weights.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::sources::{CodingBlock, MeetingNote};

/// One activity of an hour report, with Clovy's minutes.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HourActivity {
    pub description: String,
    pub minutes: i64,
}

#[derive(Clone, Debug, PartialEq)]
pub struct HourAnswer {
    pub summary: String,
    /// Descriptions with the model's minutes (weights).
    pub activities: Vec<(String, f64)>,
}

/// Splits `active_minutes` across activities in proportion to `weights`
/// (largest remainder, so the parts add up exactly). Negative or missing
/// weights count as zero; all-zero weights split evenly.
pub fn normalize_minutes(active_minutes: i64, weights: &[f64]) -> Vec<i64> {
    if weights.is_empty() || active_minutes <= 0 {
        return vec![0; weights.len()];
    }
    let clean: Vec<f64> = weights
        .iter()
        .map(|weight| {
            if weight.is_finite() && *weight > 0.0 {
                *weight
            } else {
                0.0
            }
        })
        .collect();
    let total: f64 = clean.iter().sum();
    let shares: Vec<f64> = if total > 0.0 {
        clean.iter().map(|weight| weight / total).collect()
    } else {
        vec![1.0 / clean.len() as f64; clean.len()]
    };
    let exact: Vec<f64> = shares
        .iter()
        .map(|share| share * active_minutes as f64)
        .collect();
    let mut minutes: Vec<i64> = exact.iter().map(|value| value.floor() as i64).collect();
    let mut remaining = active_minutes - minutes.iter().sum::<i64>();
    let mut order: Vec<usize> = (0..exact.len()).collect();
    order.sort_by(|&a, &b| {
        let fraction = |index: usize| exact[index] - exact[index].floor();
        fraction(b).total_cmp(&fraction(a)).then(a.cmp(&b))
    });
    for index in order {
        if remaining == 0 {
            break;
        }
        minutes[index] += 1;
        remaining -= 1;
    }
    minutes
}

/// The model's answer, or `None` when it has no usable activity.
pub fn parse_answer(value: &Value) -> Option<HourAnswer> {
    let summary = value.get("summary")?.as_str()?.trim().to_string();
    let activities: Vec<(String, f64)> = value
        .get("activities")?
        .as_array()?
        .iter()
        .filter_map(|activity| {
            let description = activity.get("description")?.as_str()?.trim();
            let minutes = activity
                .get("minutes")
                .and_then(Value::as_f64)
                .unwrap_or(0.0);
            (!description.is_empty()).then(|| (description.to_string(), minutes))
        })
        .collect();
    (!activities.is_empty()).then_some(HourAnswer {
        summary,
        activities,
    })
}

/// The answer's activities with Clovy's minutes, in the model's order.
pub fn assemble(answer: &HourAnswer, active_minutes: i64) -> Vec<HourActivity> {
    let weights: Vec<f64> = answer
        .activities
        .iter()
        .map(|(_, minutes)| *minutes)
        .collect();
    let minutes = normalize_minutes(active_minutes, &weights);
    answer
        .activities
        .iter()
        .zip(minutes)
        .map(|((description, _), minutes)| HourActivity {
            description: description.clone(),
            minutes,
        })
        .collect()
}

/// A session of the hour as the prompt lists it.
#[derive(Clone, Debug, PartialEq)]
pub struct SessionLine {
    pub start: String,
    pub end: String,
    pub app: String,
    pub context: Option<String>,
    pub category: String,
    pub minutes: i64,
    pub window: Option<String>,
}

pub struct HourInput<'a> {
    /// Local "YYYY-MM-DD HH:00".
    pub label: &'a str,
    pub active_minutes: i64,
    pub sessions: &'a [SessionLine],
    pub coding_blocks: &'a [CodingBlock],
    pub meetings: &'a [MeetingNote],
    pub distilled: &'a str,
}

pub fn prompt(input: &HourInput<'_>) -> String {
    let mut out = format!(
        "=== HOUR {} (local time) ===\nMeasured active time: {} min\n\nSessions (measured):\n",
        input.label, input.active_minutes
    );
    for session in input.sessions {
        let context = session
            .context
            .as_deref()
            .map(|context| format!(" · {context}"))
            .unwrap_or_default();
        let window = session
            .window
            .as_deref()
            .map(|window| format!(" · \"{window}\""))
            .unwrap_or_default();
        out.push_str(&format!(
            "- {}-{} · {}{} · {} · {} min{}\n",
            session.start,
            session.end,
            session.app,
            context,
            session.category,
            session.minutes,
            window
        ));
    }
    if !input.coding_blocks.is_empty() {
        out.push_str("\nCoding-agent sessions:\n");
        for block in input.coding_blocks {
            out.push_str(&format!("- {}\n", block.prompt_line()));
        }
    }
    if !input.meetings.is_empty() {
        out.push_str("\nMeetings and notes recorded in Clovy:\n");
        for meeting in input.meetings {
            out.push_str(&format!("- {}\n", meeting.prompt_line()));
        }
    }
    out.push_str("\nScreen text (distilled):\n");
    out.push_str(if input.distilled.is_empty() {
        "(none)"
    } else {
        input.distilled
    });
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn measured_minutes_are_split_by_the_model_weights() {
        assert_eq!(normalize_minutes(42, &[30.0, 30.0]), vec![21, 21]);
        assert_eq!(normalize_minutes(50, &[45.0, 9.0, 6.0]), vec![38, 7, 5]);
        assert_eq!(
            normalize_minutes(10, &[1.0, 1.0, 1.0]).iter().sum::<i64>(),
            10
        );
        assert_eq!(normalize_minutes(9, &[0.0, -3.0, f64::NAN]), vec![3, 3, 3]);
        assert_eq!(normalize_minutes(0, &[5.0]), vec![0]);
    }

    #[test]
    fn answers_without_activities_are_unusable() {
        assert!(parse_answer(&json!({"summary": "x", "activities": []})).is_none());
        let answer = parse_answer(&json!({
            "summary": " Fixed the login bug. ",
            "activities": [
                {"description": "Fixed the login bug", "minutes": 30},
                {"description": "  ", "minutes": 5},
                {"description": "Reviewed a PR", "minutes": 30}
            ]
        }))
        .unwrap();
        assert_eq!(answer.summary, "Fixed the login bug.");
        let activities = assemble(&answer, 42);
        assert_eq!(
            activities,
            vec![
                HourActivity {
                    description: "Fixed the login bug".into(),
                    minutes: 21
                },
                HourActivity {
                    description: "Reviewed a PR".into(),
                    minutes: 21
                },
            ]
        );
    }
}
