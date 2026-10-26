-- World of Farmcraft's hotbar: the main action bar holds the farmer's seeds, one slot lit as the
-- pick. The mouse wheel moves the pick (Shift and the wheel zoom the camera), a number key or a
-- click picks a slot, and the server is told with ".sel <slot>"; a right-click on soil sows it.

FARMCRAFT_SLOT = 1

local function farm_light()
	for i = 1, 12 do
		local button = getglobal("ActionButton"..i)
		if button then
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

-- A press of a bar button, by key or by click, picks its slot instead of using the item.
UseAction = function(slot)
	Farmcraft_Select(slot)
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

farm_light()
