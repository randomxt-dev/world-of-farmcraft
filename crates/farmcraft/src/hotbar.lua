-- World of Farmcraft's hotbar: one button for every thing the farmer can do. Slots 1-7 are tools
-- (Till Soil, Watering Can, Sickle, Axe, Mining Pick, Build Fence, Horse), each there once a quest
-- or Brog's shop gives it; slots 8-12 hold the seeds carried. One slot is lit as the pick: the
-- mouse wheel moves it (Shift and the wheel zoom the camera), a number key or a click picks a
-- slot, and the server is told with ".sel <slot>". The farm picks the tool by itself as well: a
-- right-click on a dry crop takes the can, on a ripe one the sickle, on a tree the axe, and the
-- server lights that slot ("FarmcraftTool"). Till Soil and Build Fence cast at once when pressed,
-- the Horse sends ".ride" (the client won't cast while mounted, so a cast could never step down).
-- Also here: the bar above the hotbar shows the lit slot's skill, the farm's own names on
-- borrowed skill lines and cast bars, and every quest tracked on screen.

FARMCRAFT_SLOT = 8
local RIDE_SLOT = 7

-- The tools: icon, name, tooltip, and whether pressing the button acts (casts) or only picks.
-- The server puts borrowed spells on these buttons; these are the faces and words they wear.
local TOOLS = {
	[1] = {
		"Interface\\Icons\\INV_Misc_Shovel_01",
		"Till Soil",
		"Click a furrow in the field to dig a new plot mound.",
		true,
	},
	[2] = {
		"Interface\\Icons\\INV_Drink_Waterskin_01",
		"Watering Can",
		"Comes out by itself when you right-click a crop that needs water. Rain waters for you.",
		false,
	},
	[3] = {
		"Interface\\Icons\\INV_Misc_Herb_07",
		"Sickle",
		"Comes out by itself when you right-click a ripe crop or a wild herb.",
		false,
	},
	[4] = {
		"Interface\\Icons\\INV_Axe_01",
		"Axe",
		"Comes out by itself when you right-click a Westfall oak: chop it down for Simple Wood.",
		false,
	},
	[5] = {
		"Interface\\Icons\\INV_Pick_02",
		"Mining Pick",
		"Comes out by itself when you right-click a copper vein in Fargodeep Mine.",
		false,
	},
	[6] = {
		"Interface\\Icons\\INV_TradeskillItem_01",
		"Build Fence",
		"Click where a fence starts, then where it runs to: sections go up along the line for 2 Simple Wood each, and keep going from the end. Click the end again to stop. Short of wood, the rest is marked out with posts: right-click a post to build it later.",
		true,
	},
	[7] = {
		"Interface\\Icons\\Ability_Mount_RidingHorse",
		"Farm Horse",
		"Mount up, or press again to step down. Biscuit waits by the house when you're on foot.",
		true,
	},
}

-- Seeds wear their crop's own icon; a little seed sack in the corner tells them from the crop.
local SEED_BADGE = "Interface\\Icons\\INV_Misc_Food_Wheat_02"

local function badge(button, show)
	local tex = button.farmcraftBadge
	if not tex then
		tex = button:CreateTexture(nil, "OVERLAY")
		tex:SetTexture(SEED_BADGE)
		tex:SetWidth(18)
		tex:SetHeight(18)
		tex:SetPoint("TOPLEFT", button, "TOPLEFT", 1, -1)
		button.farmcraftBadge = tex
	end
	if show then
		tex:Show()
	else
		tex:Hide()
	end
end

local function farm_light()
	for i = 1, 12 do
		local button = getglobal("ActionButton"..i)
		if button then
			if TOOLS[i] then
				badge(button, nil)
				-- A tool not yet earned leaves its slot empty.
				if HasAction(i) then
					getglobal("ActionButton"..i.."Icon"):SetTexture(TOOLS[i][1])
				end
			else
				badge(button, HasAction(i))
			end
			if i == FARMCRAFT_SLOT then
				button:SetChecked(1)
			else
				button:SetChecked(nil)
			end
		end
	end
end

function Farmcraft_Select(slot)
	if slot < 1 then
		slot = 12
	elseif slot > 12 then
		slot = 1
	end
	FARMCRAFT_SLOT = slot
	farm_light()
	if Farmcraft_SkillBar then
		Farmcraft_SkillBar()
	end
	SendChatMessage(".sel "..slot, "SAY")
end

-- One notch: the next slot that holds something, wrapping around the bar.
function Farmcraft_Scroll(step)
	local slot = FARMCRAFT_SLOT
	for _ = 1, 12 do
		slot = slot + step
		if slot < 1 then
			slot = 12
		elseif slot > 12 then
			slot = 1
		end
		if HasAction(slot) then
			break
		end
	end
	Farmcraft_Select(slot)
end

-- The wheel's bindings call these by name, so taking the names over takes the wheel.
local zoom_in, zoom_out = CameraZoomIn, CameraZoomOut
CameraZoomIn = function(yards)
	if IsShiftKeyDown() then
		zoom_in(yards)
	else
		Farmcraft_Scroll(-1)
	end
end
CameraZoomOut = function(yards)
	if IsShiftKeyDown() then
		zoom_out(yards)
	else
		Farmcraft_Scroll(1)
	end
end
SetBinding("SHIFT-MOUSEWHEELUP", "CAMERAZOOMIN")
SetBinding("SHIFT-MOUSEWHEELDOWN", "CAMERAZOOMOUT")

-- A press of a bar button, by key or by click: the acting tools cast (Till Soil and Build Fence
-- bring up the ground circle), everything else becomes the pick.
local use_action = UseAction
UseAction = function(slot, check_cursor, on_self)
	if slot == RIDE_SLOT then
		if HasAction(RIDE_SLOT) then
			SendChatMessage(".ride", "SAY")
		end
		return
	end
	if TOOLS[slot] and TOOLS[slot][4] then
		return use_action(slot, check_cursor, on_self)
	end
	Farmcraft_Select(slot)
end

-- The tools' tooltips name what they do, not the borrowed spell.
local set_tooltip = ActionButton_SetTooltip
ActionButton_SetTooltip = function()
	local tool = TOOLS[ActionButton_GetPagedID(this)]
	if not tool then
		return set_tooltip()
	end
	GameTooltip:SetOwner(this, "ANCHOR_RIGHT")
	GameTooltip:SetText(tool[2])
	GameTooltip:AddLine(tool[3], 1, 1, 1, 1)
	GameTooltip:Show()
end

-- A button's full update puts the spell's icon back; the tools keep theirs.
local update = ActionButton_Update
ActionButton_Update = function()
	update()
	farm_light()
end

-- The stock button code lights the button of the action being cast; keep the pick lit instead.
local update_state = ActionButton_UpdateState
ActionButton_UpdateState = function()
	update_state()
	farm_light()
end
local button_up = ActionButtonUp
ActionButtonUp = function(id, on_self)
	button_up(id, on_self)
	farm_light()
end

-- The bags: a badge on every stack of seeds.
local container_update = ContainerFrame_Update
ContainerFrame_Update = function(frame)
	container_update(frame)
	local bag = frame:GetID()
	for j = 1, frame.size or 0 do
		local button = getglobal(frame:GetName().."Item"..j)
		if button then
			local link = GetContainerItemLink(bag, button:GetID())
			badge(button, link ~= nil and string.find(link, "Seeds%]") ~= nil)
		end
	end
end

-- The skills the farm adds wear borrowed skill lines; these are their own names.
local SKILL_NAMES = {
	["Tailoring"] = "Farming",
	["Skinning"] = "Lumberjacking",
	["Engineering"] = "Woodworking",
	["Leatherworking"] = "Shepherding",
}
local get_skill_line = GetSkillLineInfo
GetSkillLineInfo = function(i)
	local name, header, expanded, rank, temp, modifier, max, abandon, step, cost, level, kind, text =
		get_skill_line(i)
	return SKILL_NAMES[name] or name, header, expanded, rank, temp, modifier, max, abandon, step,
		cost, level, kind, text
end
local chat_event = ChatFrame_OnEvent
ChatFrame_OnEvent = function(e)
	if e == "CHAT_MSG_SKILL" and arg1 then
		for from, to in pairs(SKILL_NAMES) do
			arg1 = string.gsub(arg1, from, to)
		end
	end
	return chat_event(e)
end

-- The experience bar above the hotbar shows the skill of the picked slot: Farming for the soil,
-- the can and the seeds, Lumberjacking for the axe, Woodworking for fences, Riding for the horse.
local SLOT_SKILLS = {
	[1] = "Farming",
	[2] = "Farming",
	[3] = "Herbalism",
	[4] = "Lumberjacking",
	[5] = "Mining",
	[6] = "Woodworking",
	[7] = "Riding",
}
local function slot_skill(slot)
	return SLOT_SKILLS[slot] or (slot > RIDE_SLOT and "Farming") or nil
end
local function skill_level(name)
	for i = 1, GetNumSkillLines() do
		local n, header, _, rank, _, _, max = GetSkillLineInfo(i)
		if not header and n == name then
			return rank, max
		end
	end
end
local exp_update = MainMenuExpBar_Update
function Farmcraft_SkillBar()
	local name = slot_skill(FARMCRAFT_SLOT)
	local rank, max = nil, nil
	if name then
		rank, max = skill_level(name)
	end
	if not rank then
		return exp_update()
	end
	MainMenuExpBar:SetMinMaxValues(0, max)
	MainMenuExpBar:SetValue(rank)
	MainMenuExpBar:SetStatusBarColor(0.85, 0.65, 0.1)
	if ExhaustionTick then
		ExhaustionTick:Hide()
	end
	if MainMenuBarExpText then
		MainMenuBarExpText:SetText(name.." "..rank.." / "..max)
		MainMenuBarExpText:Show()
	end
end
MainMenuExpBar_Update = Farmcraft_SkillBar
local skill_watch = CreateFrame("Frame")
skill_watch:RegisterEvent("SKILL_LINES_CHANGED")
skill_watch:RegisterEvent("PLAYER_XP_UPDATE")
skill_watch:RegisterEvent("CHAT_MSG_ADDON")
skill_watch:SetScript("OnEvent", function()
	-- The server lights the tool it took for a job; this is not a pick to tell it about.
	if event == "CHAT_MSG_ADDON" then
		if arg1 ~= "FarmcraftTool" then
			return
		end
		FARMCRAFT_SLOT = tonumber(arg2) or FARMCRAFT_SLOT
		farm_light()
	end
	Farmcraft_SkillBar()
end)

-- Cast bars of borrowed spells, named for what the farmer is doing.
local CAST_NAMES = {
	["Gazban Water Form"] = "Watering",
}
local cast_event = CastingBarFrame_OnEvent
CastingBarFrame_OnEvent = function()
	cast_event()
	if event == "SPELLCAST_START" and CAST_NAMES[arg1] then
		local text = CastingBarText or CastingBarFrameText
		if text then
			text:SetText(CAST_NAMES[arg1])
		end
	end
end

-- Every quest in the log is tracked on screen, under the minimap.
local function watch_all()
	for i = 1, GetNumQuestLogEntries() do
		local _, _, _, header = GetQuestLogTitle(i)
		if not header and not IsQuestWatched(i) then
			AddQuestWatch(i)
		end
	end
	QuestWatch_Update()
end
local watcher = CreateFrame("Frame")
watcher:RegisterEvent("QUEST_LOG_UPDATE")
watcher:SetScript("OnEvent", watch_all)
watch_all()

farm_light()

Farmcraft_SkillBar()
