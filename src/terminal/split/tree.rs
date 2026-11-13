//! Split pane-tree conversion and insertion.

// Rust guideline compliant 2026-10-09

use iced::widget::pane_grid;

use crate::app::SessionId;
use crate::terminal::Split;

pub(in crate::terminal) fn node_to_configuration(
    node: &pane_grid::Node,
    grid: &pane_grid::State<SessionId>,
) -> pane_grid::Configuration<SessionId> {
    match node {
        pane_grid::Node::Split {
            axis, ratio, a, b, ..
        } => pane_grid::Configuration::Split {
            axis: *axis,
            ratio: *ratio,
            a: Box::new(node_to_configuration(a, grid)),
            b: Box::new(node_to_configuration(b, grid)),
        },
        pane_grid::Node::Pane(pane) => pane_grid::Configuration::Pane(
            *grid
                .get(*pane)
                .expect("pane-grid layout references a missing pane"),
        ),
    }
}

pub(in crate::terminal) fn pane_for_session(
    panes: &pane_grid::State<SessionId>,
    session_id: SessionId,
) -> Option<pane_grid::Pane> {
    panes
        .iter()
        .find(|(_, sid)| **sid == session_id)
        .map(|(pane, _)| *pane)
}

pub(in crate::terminal) fn split_configuration(
    split: &Split,
) -> pane_grid::Configuration<SessionId> {
    node_to_configuration(split.panes.layout(), &split.panes)
}

pub(in crate::terminal) fn inject(
    config: pane_grid::Configuration<SessionId>,
    target_session: SessionId,
    split_axis: pane_grid::Axis,
    place_before_target: bool,
    dragged: &pane_grid::Configuration<SessionId>,
) -> pane_grid::Configuration<SessionId> {
    match config {
        pane_grid::Configuration::Pane(sid) if sid == target_session => {
            let target = pane_grid::Configuration::Pane(sid);
            let (a, b) = if place_before_target {
                (dragged.clone(), target)
            } else {
                (target, dragged.clone())
            };
            pane_grid::Configuration::Split {
                axis: split_axis,
                ratio: 0.5,
                a: Box::new(a),
                b: Box::new(b),
            }
        }
        pane_grid::Configuration::Pane(sid) => pane_grid::Configuration::Pane(sid),
        pane_grid::Configuration::Split { axis, ratio, a, b } => pane_grid::Configuration::Split {
            axis,
            ratio,
            a: Box::new(inject(
                *a,
                target_session,
                split_axis,
                place_before_target,
                dragged,
            )),
            b: Box::new(inject(
                *b,
                target_session,
                split_axis,
                place_before_target,
                dragged,
            )),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn configuration(sessions: &[SessionId]) -> pane_grid::Configuration<SessionId> {
        match sessions {
            [only] => pane_grid::Configuration::Pane(*only),
            [first, rest @ ..] => pane_grid::Configuration::Split {
                axis: pane_grid::Axis::Vertical,
                ratio: 0.5,
                a: Box::new(pane_grid::Configuration::Pane(*first)),
                b: Box::new(configuration(rest)),
            },
            [] => panic!("a split needs at least one session"),
        }
    }

    fn flatten(config: &pane_grid::Configuration<SessionId>) -> Vec<SessionId> {
        match config {
            pane_grid::Configuration::Pane(session) => vec![*session],
            pane_grid::Configuration::Split { a, b, .. } => {
                let mut ids = flatten(a);
                ids.extend(flatten(b));
                ids
            }
        }
    }

    #[test]
    fn injecting_a_split_wraps_only_the_pane_it_was_dropped_on() {
        let dragged = pane_grid::Configuration::Pane(SessionId(9));
        let tree = configuration(&[SessionId(1), SessionId(2), SessionId(3)]);

        let merged = inject(
            tree,
            SessionId(2),
            pane_grid::Axis::Horizontal,
            true,
            &dragged,
        );

        assert_eq!(
            flatten(&merged),
            vec![SessionId(1), SessionId(9), SessionId(2), SessionId(3)],
            "the dragged pane lands beside its target and the rest of the tree is untouched"
        );
    }
}
