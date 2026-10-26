//! Reports, emergency meetings, discussion and voting, the tally, ejection and what follows.

mod common;

use std::time::Duration;

use among_rules::*;
use common::*;

const DISCUSSION: Duration = Duration::from_secs(15);
const VOTING: Duration = Duration::from_secs(120);

/// A game of `n` (1 impostor up to 6, 2 from 7) with everyone at the button.
fn at_button(settings: Settings, n: u64) -> (Game, Vec<PlayerId>, Vec<PlayerId>) {
    let mut g = started_with(settings, n, 21);
    for i in 1..=n {
        put(&mut g, id(i), BUTTON);
    }
    let (imps, crew) = (impostors(&g), crew(&g));
    (g, imps, crew)
}

/// Calls an emergency meeting and skips the discussion.
fn to_voting(g: &mut Game, caller: PlayerId) {
    ok(g, Command::CallEmergency { player: caller });
    let events = tick(g, DISCUSSION.as_secs_f64());
    assert_eq!(events, [Event::VotingOpened { voting: VOTING }]);
}

fn vote(g: &mut Game, voter: PlayerId, vote: Vote) -> Vec<Event> {
    ok(g, Command::Vote { voter, vote })
}

fn closed(events: &[Event]) -> Option<VoteOutcome> {
    events.iter().find_map(|e| match e {
        Event::VotingClosed { outcome, .. } => Some(*outcome),
        _ => None,
    })
}

#[test]
fn reporting_a_body_in_reach_calls_a_meeting_and_clears_every_body() {
    let (mut g, imps, crew) = at_button(instant(), 6);
    ok(
        &mut g,
        Command::Kill {
            killer: imps[0],
            victim: crew[0],
        },
    );
    put(&mut g, crew[0], [50.0, 0.0, 0.0]);
    put(&mut g, crew[1], [8.5, 0.0, 0.0]);
    let report = |player| Command::Report {
        player,
        body: crew[0],
    };
    assert_eq!(rejected(&mut g, report(crew[1])), Reject::OutOfRange);
    assert_eq!(rejected(&mut g, report(crew[0])), Reject::Dead);
    assert_eq!(
        rejected(
            &mut g,
            Command::Report {
                player: crew[2],
                body: crew[2]
            }
        ),
        Reject::NoSuchBody
    );
    put(&mut g, crew[1], [8.0, 0.0, 0.0]);
    let events = ok(&mut g, report(crew[1]));
    assert_eq!(
        events,
        [
            Event::MeetingCalled {
                caller: crew[1],
                reason: MeetingReason::Report { body: crew[0] },
                discussion: DISCUSSION,
            },
            Event::BodiesCleared,
        ]
    );
    assert!(g.bodies().is_empty());
    assert!(matches!(g.phase(), Phase::Meeting(m) if m.stage == MeetingStage::Discussion));
}

#[test]
fn the_emergency_button_has_reach_a_cooldown_and_a_per_player_budget() {
    let (mut g, _, crew) = at_button(Settings::default(), 5);
    let call = |player| Command::CallEmergency { player };
    assert_eq!(rejected(&mut g, call(crew[0])), Reject::OnCooldown);
    tick(&mut g, 15.0);
    put(&mut g, crew[0], [5.5, 0.0, 0.0]);
    assert_eq!(rejected(&mut g, call(crew[0])), Reject::OutOfRange);
    put(&mut g, crew[0], BUTTON);
    let events = ok(&mut g, call(crew[0]));
    assert_eq!(
        events,
        [Event::MeetingCalled {
            caller: crew[0],
            reason: MeetingReason::Emergency,
            discussion: DISCUSSION,
        }]
    );
    tick(&mut g, 15.0 + 120.0 + 6.0);
    assert_eq!(g.phase(), &Phase::Playing);
    assert_eq!(g.emergency_cooldown(), Duration::from_secs(15));
    tick(&mut g, 15.0);
    assert_eq!(
        rejected(&mut g, call(crew[0])),
        Reject::NoEmergencyMeetingsLeft
    );
    ok(&mut g, call(crew[1]));
}

#[test]
fn nothing_but_talk_and_votes_happens_in_a_meeting() {
    let (mut g, imps, crew) = at_button(instant(), 5);
    ok(&mut g, Command::CallEmergency { player: crew[0] });
    assert_eq!(
        rejected(
            &mut g,
            Command::Kill {
                killer: imps[0],
                victim: crew[1]
            }
        ),
        Reject::WrongPhase
    );
    assert_eq!(
        rejected(&mut g, Command::CallEmergency { player: crew[1] }),
        Reject::WrongPhase
    );
    let task = g.player(crew[1]).unwrap().tasks[0].task;
    assert_eq!(
        rejected(
            &mut g,
            Command::UseTask {
                player: crew[1],
                task
            }
        ),
        Reject::WrongPhase
    );
    assert_eq!(
        rejected(
            &mut g,
            Command::Vote {
                voter: crew[1],
                vote: Vote::Skip
            }
        ),
        Reject::WrongPhase,
        "no votes during discussion"
    );
}

#[test]
fn each_living_player_votes_once_for_a_living_player_or_skip() {
    let (mut g, imps, crew) = at_button(instant(), 6);
    ok(
        &mut g,
        Command::Kill {
            killer: imps[0],
            victim: crew[0],
        },
    );
    to_voting(&mut g, crew[1]);
    let cmd = |voter, vote| Command::Vote { voter, vote };
    assert_eq!(rejected(&mut g, cmd(crew[0], Vote::Skip)), Reject::Dead);
    assert_eq!(
        rejected(&mut g, cmd(crew[1], Vote::Player(crew[0]))),
        Reject::InvalidTarget
    );
    assert_eq!(
        vote(&mut g, crew[1], Vote::Player(crew[1])),
        [Event::VoteCast { voter: crew[1] }]
    );
    assert_eq!(
        rejected(&mut g, cmd(crew[1], Vote::Skip)),
        Reject::AlreadyVoted
    );
}

#[test]
fn a_majority_ejects_and_play_resumes_after_the_reveal() {
    let (mut g, imps, crew) = at_button(instant(), 6);
    to_voting(&mut g, crew[0]);
    let target = crew[3];
    let voters: Vec<_> = g.players().map(|(id, _)| id).collect();
    let mut last = Vec::new();
    for (i, &v) in voters.iter().enumerate() {
        last = vote(
            &mut g,
            v,
            if i == 0 {
                Vote::Skip
            } else {
                Vote::Player(target)
            },
        );
    }
    // The last vote closes voting early.
    let expected_votes: Vec<_> = (voters.iter().enumerate())
        .map(|(i, &v)| {
            (
                v,
                if i == 0 {
                    Vote::Skip
                } else {
                    Vote::Player(target)
                },
            )
        })
        .collect();
    assert_eq!(
        last,
        [
            Event::VoteCast { voter: voters[5] },
            Event::VotingClosed {
                votes: expected_votes,
                outcome: VoteOutcome::Ejected(target)
            },
            Event::Ejected {
                player: target,
                impostor: Some(false),
                impostors_left: Some(1)
            },
        ]
    );
    assert!(!g.player(target).unwrap().alive);
    assert!(matches!(g.phase(), Phase::Ejecting { ejected: Some(t), .. } if *t == target));
    assert_eq!(tick(&mut g, 5.9), []);
    assert_eq!(tick(&mut g, 0.1), [Event::MeetingEnded]);
    assert_eq!(g.phase(), &Phase::Playing);
    assert_eq!(
        g.player(imps[0]).unwrap().kill_cooldown,
        Settings::default().kill_cooldown
    );
}

#[test]
fn the_tally_needs_a_strict_lead_over_every_player_and_skip() {
    let outcome = |ballots: &[Option<usize>]| {
        let (mut g, _, crew) = at_button(instant(), 6);
        to_voting(&mut g, crew[0]);
        for (voter, ballot) in crew.iter().zip(ballots) {
            if let Some(b) = ballot {
                let v = if *b == 99 {
                    Vote::Skip
                } else {
                    Vote::Player(crew[*b])
                };
                vote(&mut g, *voter, v);
            }
        }
        closed(&tick(&mut g, VOTING.as_secs_f64())).unwrap()
    };
    let (skip, none) = (Some(99), None);
    let crew_of = |i| at_button(instant(), 6).2[i];
    assert_eq!(
        outcome(&[Some(1), Some(1), Some(2), none, none]),
        VoteOutcome::Ejected(crew_of(1))
    );
    assert_eq!(
        outcome(&[Some(1), Some(1), Some(2), Some(2), none]),
        VoteOutcome::Tie
    );
    assert_eq!(
        outcome(&[Some(1), Some(1), skip, skip, none]),
        VoteOutcome::Tie
    );
    assert_eq!(
        outcome(&[Some(1), skip, skip, none, none]),
        VoteOutcome::Skipped
    );
    assert_eq!(
        outcome(&[none, none, none, none, none]),
        VoteOutcome::Skipped
    );
}

#[test]
fn ejecting_the_last_impostor_wins_for_the_crew_after_the_reveal() {
    let (mut g, imps, crew) = at_button(instant(), 5);
    to_voting(&mut g, crew[0]);
    for &c in &crew {
        vote(&mut g, c, Vote::Player(imps[0]));
    }
    let events = vote(&mut g, imps[0], Vote::Skip);
    assert!(events.contains(&Event::Ejected {
        player: imps[0],
        impostor: Some(true),
        impostors_left: Some(0),
    }));
    assert_eq!(game_over(&events), None, "the reveal plays first");
    let events = tick(&mut g, 6.0);
    assert_eq!(events[0], Event::MeetingEnded);
    assert_eq!(
        game_over(&events),
        Some((Team::Crew, WinReason::ImpostorsEliminated))
    );
}

#[test]
fn ejecting_a_crewmate_can_hand_the_impostors_the_win() {
    let (mut g, imps, crew) = at_button(instant(), 4);
    ok(
        &mut g,
        Command::Kill {
            killer: imps[0],
            victim: crew[0],
        },
    );
    to_voting(&mut g, crew[1]);
    for v in [crew[1], crew[2], imps[0]] {
        vote(&mut g, v, Vote::Player(crew[2]));
    }
    let events = tick(&mut g, 6.0);
    assert_eq!(
        game_over(&events),
        Some((Team::Impostors, WinReason::Outnumbered))
    );
}

#[test]
fn without_confirm_ejects_the_ejection_reveals_nothing() {
    let (mut g, imps, crew) = at_button(
        Settings {
            confirm_ejects: false,
            ..instant()
        },
        5,
    );
    to_voting(&mut g, crew[0]);
    let all: Vec<_> = g.players().map(|(id, _)| id).collect();
    let mut events = Vec::new();
    for v in all {
        events = vote(&mut g, v, Vote::Player(imps[0]));
    }
    assert!(events.contains(&Event::Ejected {
        player: imps[0],
        impostor: None,
        impostors_left: None,
    }));
}

#[test]
fn one_long_tick_runs_every_stage_in_order() {
    let settings = Settings {
        emergency_cooldown: Duration::from_secs(3),
        ..instant()
    };
    let (mut g, _, crew) = at_button(settings, 5);
    tick(&mut g, 3.0);
    ok(&mut g, Command::CallEmergency { player: crew[0] });
    let events = tick(&mut g, 15.0 + 120.0 + 6.0 + 1.0);
    assert_eq!(
        events,
        [
            Event::VotingOpened { voting: VOTING },
            Event::VotingClosed {
                votes: vec![],
                outcome: VoteOutcome::Skipped
            },
            Event::MeetingEnded,
        ]
    );
    assert_eq!(g.phase(), &Phase::Playing);
    // The second left over after the reveal already counts against the emergency cooldown.
    assert_eq!(g.emergency_cooldown(), Duration::from_secs(2));
}

#[test]
fn no_discussion_time_opens_voting_at_once() {
    let settings = Settings {
        discussion_time: Duration::ZERO,
        ..instant()
    };
    let (mut g, _, crew) = at_button(settings, 5);
    let events = ok(&mut g, Command::CallEmergency { player: crew[0] });
    assert_eq!(events[1], Event::VotingOpened { voting: VOTING });
    vote(&mut g, crew[1], Vote::Skip);
}

#[test]
fn leaving_mid_vote_returns_the_votes_cast_for_them() {
    let (mut g, imps, crew) = at_button(instant(), 6);
    to_voting(&mut g, crew[0]);
    vote(&mut g, crew[0], Vote::Player(crew[4]));
    vote(&mut g, crew[1], Vote::Skip);
    let events = ok(&mut g, Command::Leave { player: crew[4] });
    assert!(events.contains(&Event::VoteReturned { voter: crew[0] }));
    let Phase::Meeting(m) = g.phase() else {
        panic!()
    };
    assert!(!m.votes.contains_key(&crew[0]));
    vote(&mut g, crew[0], Vote::Skip);
    vote(&mut g, crew[2], Vote::Skip);
    vote(&mut g, crew[3], Vote::Skip);
    // The impostor is the last connected living voter: their vote closes it.
    let events = vote(&mut g, imps[0], Vote::Skip);
    assert_eq!(closed(&events), Some(VoteOutcome::Skipped));
}
