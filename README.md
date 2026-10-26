# WOWGameX

Created by Masterbot on 2026-10-26 for a Claude Code agent.

## Brief

I want to build a game that uses World of Warcraft, the way Azeroth Survivors did. I don't know what yet. First job: brainstorm game ideas with me. Don't build anything yet.

Research so far (from Masterbot, Oct 26 2026):

Azeroth Survivors (play: https://wow.aethus.pl/, source: https://github.com/OutThisLife/azeroth-survivors-re, 315 stars) is a Vampire Survivors style game set in classic WoW. You pick a class and one of 16 zones, spells auto-fire, you level up and choose cards. Each zone uses its own creatures and bosses. Top-down camera, C unlocks it. Offline: no server, no login, no co-op.

How it is built:
- Engine: benilla (https://github.com/samwhosung/benilla, 826 stars, ~1000 commits, 'complete and fully playable'). A from-scratch WoW 1.12.1 (vanilla) client in Rust + Bevy 0.18 (wgpu 27). It reads the original game files directly: MPQ, BLP, DBC, ADT/WDT terrain, WMO buildings, M2 animated models, particles, spell visuals. Its format readers and UI core have no Bevy in them, and the world renderer runs with no game attached, so it is reusable. The survivors mode is a module inside the app crate: crates/benilla-app/src/survivors/.
- Browser: wenilla (https://github.com/Arnesen/wenilla) is benilla compiled to WebAssembly. WebGPU is required.
- Code license: MIT OR Apache-2.0. No Blizzard files in the repos.
- Local build: scripts/fetch-data.sh downloads the English 1.12.1 client (build 5875, ~5.5 GB) from archive.org into wow-data/Data/ (or set WOW_DATA / --data to an existing install). make build, then make start serves http://127.0.0.1:8090/. On Windows run everything from Git Bash; PowerShell and cmd can't run the scripts.
- Where creature spawns come from is not documented. DBC files don't hold spawns, so it is probably a vanilla server DB dump (vmangos/cMaNGOS). Unconfirmed.

How the hosted site loads without a 5.5 GB download (checked in its JS): it sets __wenilla_data_layout='static' and fetches individual extracted game files from https://wow.aethus.pl/data/ on demand. boot-manifest.json (a few hundred data tables and UI files) loads before the title screen, world-manifest.json on entering the world, and the rest streams as you move. Files are cached a day. So the hosted site redistributes Blizzard's files, which is the legal grey part. Fine for a fan or friends project, not for selling.

Hosting options for our game: (1) stream extracted files like aethus; (2) a trimmed pack with only the zones and models our game uses, probably 100-300 MB for a 1-2 zone game, streamed the same way (my preferred direction for a small mode); (3) the player supplies their own 1.12 client through a local gateway (no hosted Blizzard files, but almost nobody will bother).

Other benilla forks:
- World of Skatecraft: a skate game on benilla already exists. Look at it.
- world-of-kartcraft (https://github.com/duxanlol/world-of-kartcraft): a fork of World of Skatecraft. Crash Team Racing kart physics ported from the CTR decompilation, dropped into WoW 1.12.1. 'Vibe coded, just for laughs'. Not browser: needs Docker, a local vmangos server, your own CTR disc, ~20 GB. GPL-3.0.
- warcraft-zones-zero (https://github.com/ari92p/warcraft-zones-zero): a third-person WoW dungeon action prototype.

Not relevant: my AzerothCore VM (C:\Users\rando\Desktop\AZEROTHCORE_VM) is a WotLK 3.3.5 server. These projects use 1.12.1 client data and need no server.

Ideas I mentioned: a skate game in WoW (Ironforge or Orgrimmar as skateparks) or an RTS in WoW. I'm open to anything.

What I want from you now:
1. Look at World of Skatecraft and anything else built on benilla.
2. Pitch 8-12 game ideas that play to what benilla gives us (real zones, creatures, spells, animations, UI), ranging from small and quick to ambitious. For each: the hook, the core loop, rough effort, and what benilla already provides vs what we'd have to build.
3. Recommend your top 2-3 to prototype first, and say why.
Keep it readable on my phone: short sections and lists, small tables only.
