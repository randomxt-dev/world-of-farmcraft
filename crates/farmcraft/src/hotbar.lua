-- World of Farmcraft's hotbar: one button for every thing the farmer can do. Slots 1-5 are tools
-- (Till Soil, Watering Can, Axe, Build Fence, Horse), slots 6-12 the seeds carried. One slot is
-- lit as the pick: the mouse wheel moves it (Shift and the wheel zoom the camera), a number key
-- or a click picks a slot, and the server is told with ".sel <slot>". Till Soil, Build Fence and
-- the Horse act at once when pressed; the can, the axe and the seeds say what a right-click on
-- the farm does.

FARMCRAFT_SLOT = 6

-- The tools: icon, name, tooltip, and whether pressing the button acts (casts) or only picks.
-- The server puts borrowed spells on these buttons; these are the faces and words they wear.
local TOOLS = {
	[1] = {
		"Interface\\Icons\\INV_Misc_Shovel_01",
		"Till Soil",
		"Click a spot in the field to dig a new plot mound.",
		true,
	},
	[2] = {
		"Interface\\Icons\\INV_Drink_Waterskin_01",
		"Watering Can",
		"Pick this, then right-click a planted crop to water it. Rain waters for you.",
		false,
	},
	[3] = {
		"Interface\\Icons\\INV_Axe_01",
		"Axe",
		"Right-click a Westfall oak to chop it down for Simple Wood.",
		false,
	},
	[4] = {
		"Interface\\Icons\\INV_TradeskillItem_01",
		"Build Fence",
		"Click where a fence starts, then where it runs to: sections go up along the line for 2 Simple Wood each, and keep going from the end. Click the end again to stop. Short of wood, the rest is marked out with posts: right-click a post to build it later.",
		true,
	},
	[5] = {
		"Interface\\Icons\\Ability_Mount_RidingHorse",
		"Farm Horse",
		"Mount up or step down. Riding is twice as fast as walking.",
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
				getglobal("ActionButton"..i.."Icon"):SetTexture(TOOLS[i][1])
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

farm_light()
