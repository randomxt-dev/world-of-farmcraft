-- World of Farmcraft's hotbar: the main action bar holds the farmer's seeds, one slot lit as the
-- pick. The mouse wheel moves the pick (Shift and the wheel zoom the camera), a number key or a
-- click picks a slot, and the server is told with ".sel <slot>"; a right-click on soil sows it.

FARMCRAFT_SLOT = 1

-- The last two buttons are tools, not seeds: the server puts ground-targeted spells there, and
-- these are the faces and words they wear.
local TOOLS = {
	[11] = {
		"Interface\\Icons\\INV_Misc_Shovel_01",
		"Till Soil",
		"Click a spot in the field to dig a new plot mound.",
	},
	[12] = {
		"Interface\\Icons\\INV_TradeskillItem_01",
		"Build Fence",
		"Click the ground to put up a fence for 2 Simple Wood. Pick the kind at the fence gallery; right-click a fence to take it down.",
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
		if button and TOOLS[i] then
			badge(button, nil)
			getglobal("ActionButton"..i.."Icon"):SetTexture(TOOLS[i][1])
			button:SetChecked(nil)
		elseif button then
			-- The rest of the bar holds only seeds.
			badge(button, HasAction(i))
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
		if HasAction(slot) and not TOOLS[slot] then
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

-- A press of a bar button, by key or by click, picks its slot instead of using the item.
-- The tools still cast: their press brings up the ground circle.
local use_action = UseAction
UseAction = function(slot, check_cursor, on_self)
	if TOOLS[slot] then
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
