//! Class abilities on players: targets, cooldowns, each effect, traps, the kill shield, and
//! effects ending with a meeting.

mod common;

use std::time::Duration;

use among_rules::*;
use common::*;

/// No cooldowns in the way, ability ones included.
fn ready() -> Settings {
    Settings {
        initial_ability_cooldown: Duration::ZERO,
        ..instant()
    }
}

/// Players `1..` fighting as `classes`, started with one impostor, everyone at the origin.
fn game(classes: &[Class]) -> Game {
    let mut g = Game::new(ready(), layout());
    for (i, &class) in classes.iter().enumerate() {
        ok(
            &mut g,
            Command::Join {
                player: id(i as u64 + 1),
                class,
            },
        );
    }
    ok(&mut g, Command::Start { by: id(1), seed: 7 });
    let ids: Vec<_> = g.players().map(|(id, _)| id).collect();
    for p in ids {
        put(&mut g, p, [0.0; 3]);
    }
    g
}

/// Five players; player 1 is `class`, the rest warriors.
fn with(class: Class) -> Game {
    let mut classes = vec![Class::Warrior; 5];
    classes[0] = class;
    game(&classes)
}

fn on(player: PlayerId, target: PlayerId) -> Command {
    Command::UseAbility {
        player,
        target: Some(target),
    }
}

fn myself(player: PlayerId) -> Command {
    Command::UseAbility {
        player,
        target: None,
    }
}

/// A crewmate other than `not`.
fn crewmate_but(g: &Game, not: PlayerId) -> PlayerId {
    crew(g).into_iter().find(|&c| c != not).unwrap()
}

fn kill(killer: PlayerId, victim: PlayerId) -> Command {
    Command::Kill { killer, victim }
}

fn under(g: &Game, p: PlayerId, effect: Effect) -> bool {
    g.player(p).unwrap().is_under(effect)
}

#[test]
fn an_ability_waits_for_its_cooldown_and_the_first_for_the_game_start() {
    let mut g = Game::new(instant(), layout());
    for i in 1..=5 {
        let class = if i == 1 {
            Class::Shaman
        } else {
            Class::Warrior
        };
        ok(
            &mut g,
            Command::Join {
                player: id(i),
                class,
            },
        );
    }
    ok(&mut g, Command::Start { by: id(1), seed: 7 });
    assert_eq!(rejected(&mut g, myself(id(1))), Reject::OnCooldown);
    tick(&mut g, 10.0);
    let events = ok(&mut g, myself(id(1)));
    assert!(events.contains(&Event::AbilityUsed {
        player: id(1),
        ability: Ability::GhostWolf,
        target: None
    }));
    assert_eq!(rejected(&mut g, myself(id(1))), Reject::OnCooldown);
    tick(&mut g, 29.9);
    assert_eq!(rejected(&mut g, myself(id(1))), Reject::OnCooldown);
    tick(&mut g, 0.1);
    ok(&mut g, myself(id(1)));
}

#[test]
fn ghost_wolf_speeds_its_shaman_up_for_ten_seconds() {
    let mut g = with(Class::Shaman);
    ok(&mut g, myself(id(1)));
    let p = g.player(id(1)).unwrap();
    assert!(p.is_under(Effect::GhostWolf));
    assert!((p.speed() - 1.4).abs() < 1e-6);
    let events = tick(&mut g, 10.0);
    assert!(events.contains(&Event::EffectEnded {
        player: id(1),
        effect: Effect::GhostWolf
    }));
    assert_eq!(g.player(id(1)).unwrap().speed(), 1.0);
}

#[test]
fn an_ability_on_another_needs_a_living_visible_player_in_range_never_oneself() {
    let mut g = with(Class::Mage);
    assert_eq!(rejected(&mut g, on(id(1), id(1))), Reject::InvalidTarget);
    assert_eq!(rejected(&mut g, myself(id(1))), Reject::InvalidTarget);
    put(&mut g, id(2), [31.0, 0.0, 0.0]);
    assert_eq!(rejected(&mut g, on(id(1), id(2))), Reject::OutOfRange);
    put(&mut g, id(2), [29.0, 0.0, 0.0]);
    ok(&mut g, on(id(1), id(2)));
}

#[test]
fn polymorph_leaves_its_sheep_slow_and_unable_to_act_for_ten_seconds() {
    let mut g = with(Class::Mage);
    let imp = impostors(&g)[0];
    let target = if imp == id(1) {
        crewmate_but(&g, id(1))
    } else {
        imp
    };
    let events = ok(&mut g, on(id(1), target));
    assert!(events.contains(&Event::EffectStarted {
        player: target,
        effect: Effect::Polymorph,
        duration: Duration::from_secs(10),
        by: id(1)
    }));
    let p = g.player(target).unwrap();
    assert!(p.is_incapacitated() && !p.is_rooted());
    assert_eq!(p.speed(), 0.5);
    let r = rejected(&mut g, Command::CallEmergency { player: target });
    assert_eq!(r, Reject::Incapacitated);
    if target == imp {
        let victim = crewmate_but(&g, id(1));
        assert_eq!(rejected(&mut g, kill(imp, victim)), Reject::Incapacitated);
        assert_eq!(g.kill_target(imp), None);
    }
    tick(&mut g, 10.0);
    assert!(!g.player(target).unwrap().is_incapacitated());
    ok(&mut g, Command::CallEmergency { player: target });
}

#[test]
fn fear_roots_its_target_and_breaks_their_task_cast() {
    let mut g = Game::new(ready(), cast_layout(5.0));
    for i in 1..=5 {
        let class = if i == 1 {
            Class::Warlock
        } else {
            Class::Warrior
        };
        ok(
            &mut g,
            Command::Join {
                player: id(i),
                class,
            },
        );
    }
    ok(&mut g, Command::Start { by: id(1), seed: 7 });
    let target = crewmate_but(&g, id(1));
    let task = g.player(target).unwrap().tasks[0].task;
    let spot = task_spot(&g, target, task);
    put(&mut g, target, spot);
    put(&mut g, id(1), spot);
    ok(
        &mut g,
        Command::UseTask {
            player: target,
            task,
        },
    );
    let events = ok(&mut g, on(id(1), target));
    assert!(events.contains(&Event::CastInterrupted { player: target }));
    let p = g.player(target).unwrap();
    assert!(p.is_rooted() && p.is_incapacitated());
    let r = rejected(
        &mut g,
        Command::UseTask {
            player: target,
            task,
        },
    );
    assert_eq!(r, Reject::Incapacitated);
}

#[test]
fn hamstring_halves_speed_and_roots_hold_but_both_leave_their_target_able() {
    let mut g = with(Class::Warrior);
    let target = crewmate_but(&g, id(1));
    ok(&mut g, on(id(1), target));
    let p = g.player(target).unwrap();
    assert_eq!(p.speed(), 0.5);
    assert!(!p.is_incapacitated() && !p.is_rooted());

    let mut g = with(Class::Druid);
    let target = crewmate_but(&g, id(1));
    ok(&mut g, on(id(1), target));
    let p = g.player(target).unwrap();
    assert!(p.is_rooted() && !p.is_incapacitated());
    ok(&mut g, Command::CallEmergency { player: target });
}

#[test]
fn stealth_hides_the_rogue_from_kills_and_abilities_until_it_runs_out() {
    let mut classes = vec![Class::Mage; 5];
    classes[0] = Class::Rogue;
    let mut g = game(&classes);
    let imp = impostors(&g)[0];
    let rogue = id(1);
    if rogue == imp {
        // The rogue is the impostor: pick a seed's game where a crewmate is the rogue instead.
        return stealth_as_impostor(g, rogue);
    }
    ok(&mut g, myself(rogue));
    assert!(g.player(rogue).unwrap().is_hidden());
    assert_eq!(rejected(&mut g, kill(imp, rogue)), Reject::InvalidTarget);
    let mage = crewmate_but(&g, rogue);
    assert_eq!(rejected(&mut g, on(mage, rogue)), Reject::InvalidTarget);
    tick(&mut g, 29.9);
    assert!(g.player(rogue).unwrap().is_hidden());
    tick(&mut g, 0.1);
    assert!(!g.player(rogue).unwrap().is_hidden());
    ok(&mut g, kill(imp, rogue));
}

/// An impostor rogue's kill breaks their stealth.
fn stealth_as_impostor(mut g: Game, rogue: PlayerId) {
    ok(&mut g, myself(rogue));
    let victim = crew(&g)[0];
    let events = ok(&mut g, kill(rogue, victim));
    assert!(events.contains(&Event::EffectEnded {
        player: rogue,
        effect: Effect::Stealth
    }));
    assert!(!g.player(rogue).unwrap().is_hidden());
}

#[test]
fn stealth_breaks_when_the_rogue_works_a_task() {
    let mut g = with(Class::Rogue);
    let rogue = id(1);
    ok(&mut g, myself(rogue));
    let task = g.player(rogue).unwrap().tasks[0].task;
    let spot = task_spot(&g, rogue, task);
    put(&mut g, rogue, spot);
    assert!(g.player(rogue).unwrap().is_hidden(), "walking keeps it");
    let events = ok(
        &mut g,
        Command::UseTask {
            player: rogue,
            task,
        },
    );
    assert!(events.contains(&Event::EffectEnded {
        player: rogue,
        effect: Effect::Stealth
    }));
}

#[test]
fn a_creature_never_turns_on_a_stealthed_rogue() {
    let mut g = Game::new(ready(), combat_layout());
    for i in 1..=5 {
        let class = if i == 1 { Class::Rogue } else { Class::Warrior };
        ok(
            &mut g,
            Command::Join {
                player: id(i),
                class,
            },
        );
    }
    ok(&mut g, Command::Start { by: id(1), seed: 7 });
    ok(&mut g, myself(id(1)));
    put(&mut g, id(1), DEN);
    let events = tick(&mut g, 1.0);
    assert!(!events
        .iter()
        .any(|e| matches!(e, Event::CreatureAggro { .. })));
}

#[test]
fn a_shield_stops_one_kill_and_spends_it() {
    let mut classes = vec![Class::Priest; 5];
    classes[0] = Class::Priest;
    let mut g = game(&classes);
    let imp = impostors(&g)[0];
    let priest = crewmate_but(&g, imp);
    let victim = crew(&g).into_iter().find(|&c| c != priest).unwrap();
    ok(&mut g, on(priest, victim));
    assert!(under(&g, victim, Effect::Shield));
    let events = ok(&mut g, kill(imp, victim));
    assert_eq!(
        events,
        [
            Event::EffectEnded {
                player: victim,
                effect: Effect::Shield
            },
            Event::KillBlocked {
                killer: imp,
                victim,
                shielder: priest
            }
        ]
    );
    assert!(g.player(victim).unwrap().alive && g.bodies().is_empty());
    assert_eq!(
        g.player(imp).unwrap().kill_cooldown,
        Settings::default().kill_cooldown
    );
    tick(&mut g, 25.0);
    ok(&mut g, kill(imp, victim));
    assert!(!g.player(victim).unwrap().alive);
}

#[test]
fn a_priest_never_shields_themself() {
    let mut g = with(Class::Priest);
    assert_eq!(rejected(&mut g, on(id(1), id(1))), Reject::InvalidTarget);
}

#[test]
fn a_divine_shield_turns_kills_and_abilities_away_without_spending_them() {
    let mut classes = vec![Class::Paladin; 5];
    classes[1] = Class::Mage;
    let mut g = game(&classes);
    let imp = impostors(&g)[0];
    let paladin = crew(&g).into_iter().find(|&c| c != id(2)).unwrap();
    ok(&mut g, myself(paladin));
    assert_eq!(rejected(&mut g, kill(imp, paladin)), Reject::Immune);
    assert!(g.player(imp).unwrap().kill_cooldown.is_zero());
    if imp != id(2) {
        assert_eq!(rejected(&mut g, on(id(2), paladin)), Reject::Immune);
    }
    tick(&mut g, 10.0);
    ok(&mut g, kill(imp, paladin));
}

#[test]
fn a_trap_freezes_the_first_other_player_to_step_on_it() {
    let mut g = with(Class::Hunter);
    let hunter = id(1);
    let spot = [50.0, 50.0, 0.0];
    put(&mut g, hunter, spot);
    let events = ok(&mut g, myself(hunter));
    assert!(events.contains(&Event::TrapSet {
        owner: hunter,
        at: spot
    }));
    put(&mut g, hunter, [51.0, 50.0, 0.0]);
    assert_eq!(g.traps().len(), 1, "its owner never springs it");
    let walker = crewmate_but(&g, hunter);
    put(&mut g, walker, [55.0, 50.0, 0.0]);
    assert_eq!(g.traps().len(), 1, "2.5 yd is the trigger");
    let events = put(&mut g, walker, [52.0, 50.0, 0.0]);
    assert!(events.contains(&Event::TrapSprung {
        owner: hunter,
        victim: walker
    }));
    let p = g.player(walker).unwrap();
    assert!(p.is_under(Effect::Frozen) && p.is_rooted() && p.is_incapacitated());
    assert!(g.traps().is_empty());
    tick(&mut g, 10.0);
    assert!(!under(&g, walker, Effect::Frozen));
}

#[test]
fn a_trap_crumbles_after_its_lifetime_and_a_new_one_replaces_the_old() {
    let mut g = with(Class::Hunter);
    ok(&mut g, myself(id(1)));
    tick(&mut g, 45.0);
    put(&mut g, id(1), [20.0, 0.0, 0.0]);
    let events = ok(&mut g, myself(id(1)));
    assert!(events.contains(&Event::TrapGone { owner: id(1) }));
    assert_eq!(g.traps().len(), 1);
    assert_eq!(g.traps()[0].at, [20.0, 0.0, 0.0]);
    let events = tick(&mut g, 60.0);
    assert!(events.contains(&Event::TrapGone { owner: id(1) }));
    assert!(g.traps().is_empty());
}

#[test]
fn a_meeting_ends_every_effect_and_trap_and_no_ability_works_in_it() {
    let mut classes = vec![Class::Warrior; 5];
    classes[0] = Class::Mage;
    classes[1] = Class::Hunter;
    let mut g = game(&classes);
    put(&mut g, id(2), [40.0, 0.0, 0.0]);
    ok(&mut g, myself(id(2)));
    put(&mut g, id(2), [0.0; 3]);
    ok(&mut g, on(id(1), id(3)));
    let caller = (2..=5)
        .map(id)
        .find(|&p| !g.player(p).unwrap().is_incapacitated())
        .unwrap();
    let events = ok(&mut g, Command::CallEmergency { player: caller });
    assert!(events.contains(&Event::EffectEnded {
        player: id(3),
        effect: Effect::Polymorph
    }));
    assert!(events.contains(&Event::TrapGone { owner: id(2) }));
    assert!(g.players().all(|(_, p)| p.effects.is_empty()));
    assert_eq!(rejected(&mut g, on(id(1), id(4))), Reject::WrongPhase);
}

#[test]
fn the_dead_use_no_ability_and_a_death_ends_what_they_were_under() {
    let mut classes = vec![Class::Shaman; 5];
    classes[0] = Class::Shaman;
    let mut g = game(&classes);
    let imp = impostors(&g)[0];
    let victim = crew(&g)[0];
    ok(&mut g, myself(victim));
    let events = ok(&mut g, kill(imp, victim));
    assert!(events.contains(&Event::EffectEnded {
        player: victim,
        effect: Effect::GhostWolf
    }));
    tick(&mut g, 40.0);
    assert_eq!(rejected(&mut g, myself(victim)), Reject::Dead);
}
