-- World of Farmcraft's quest markers on the minimap and the world map: the yellow ! of a villager
-- with a quest to give and the ? of one to turn a quest in to, as the server sends them (an addon
-- line of "x,y,status;..." in world yards). The world map pins them on Elwynn's map; the minimap
-- places them around the player, and holds the far ones at its rim to point the way.

-- Elwynn Forest's bounds on its map (`WorldMapArea.dbc`): world y runs left to right, x top to
-- bottom.
local LEFT, RIGHT, TOP, BOTTOM = 1535.4166, -1935.4166, -7939.583, -10254.166
local WIDE, TALL = LEFT - RIGHT, TOP - BOTTOM
-- The minimap's width in yards outdoors at each zoom.
local MINIMAP_YARDS = { [0] = 466.6667, [1] = 400, [2] = 333.3333, [3] = 266.6667, [4] = 200, [5] = 133.3333 }
local ICONS = {
	[3] = "Interface\\GossipFrame\\ActiveQuestIcon",
	[5] = "Interface\\GossipFrame\\AvailableQuestIcon",
	[7] = "Interface\\GossipFrame\\ActiveQuestIcon",
}
local PREFIX = "FarmcraftMarks"

local marks = {}
local mini, world = {}, {}

local function on_map(x, y)
	return (LEFT - y) / WIDE, (TOP - x) / TALL
end

local function pin(pool, i, parent, size)
	local t = pool[i]
	if not t then
		t = parent:CreateTexture(nil, "OVERLAY")
		t:SetWidth(size)
		t:SetHeight(size)
		pool[i] = t
	end
	return t
end

-- A gray ? for a quest still under way, as over the villager's head.
local function paint(t, status)
	t:SetTexture(ICONS[status])
	if status == 3 then
		t:SetVertexColor(0.6, 0.6, 0.6)
	else
		t:SetVertexColor(1, 1, 1)
	end
end

local function hide_all(pool)
	for _, t in pairs(pool) do
		t:Hide()
	end
end

local function in_elwynn()
	return GetMapInfo() == "Elwynn"
end

local function update_world()
	hide_all(world)
	if not in_elwynn() then
		return
	end
	local w, h = WorldMapButton:GetWidth(), WorldMapButton:GetHeight()
	for i, m in ipairs(marks) do
		if ICONS[m.status] then
			local t = pin(world, i, WorldMapButton, 20)
			paint(t, m.status)
			local mx, my = on_map(m.x, m.y)
			t:ClearAllPoints()
			t:SetPoint("CENTER", WorldMapButton, "TOPLEFT", mx * w, -my * h)
			t:Show()
		end
	end
end

local function update_minimap()
	hide_all(mini)
	if WorldMapFrame:IsVisible() then
		return
	end
	SetMapToCurrentZone()
	if not in_elwynn() then
		return
	end
	local px, py = GetPlayerMapPosition("player")
	if not px or (px == 0 and py == 0) then
		return
	end
	local size = Minimap:GetWidth()
	local yards = MINIMAP_YARDS[Minimap:GetZoom()] or 266.6667
	local radius = size / 2 - 6
	for i, m in ipairs(marks) do
		if ICONS[m.status] then
			local mx, my = on_map(m.x, m.y)
			local dx = (mx - px) * WIDE / yards * size
			local dy = (my - py) * TALL / yards * size
			local d = math.sqrt(dx * dx + dy * dy)
			if d > radius then
				dx, dy = dx * radius / d, dy * radius / d
			end
			local t = pin(mini, i, Minimap, 14)
			paint(t, m.status)
			t:ClearAllPoints()
			t:SetPoint("CENTER", Minimap, "CENTER", dx, -dy)
			t:Show()
		end
	end
end

local gfind = string.gfind or string.gmatch

local frame = CreateFrame("Frame")
frame:RegisterEvent("CHAT_MSG_ADDON")
frame:RegisterEvent("WORLD_MAP_UPDATE")
frame:SetScript("OnEvent", function()
	if event == "CHAT_MSG_ADDON" and arg1 == PREFIX then
		marks = {}
		for x, y, s in gfind(arg2, "([%-%d%.]+),([%-%d%.]+),(%d+)") do
			table.insert(marks, { x = tonumber(x), y = tonumber(y), status = tonumber(s) })
		end
		update_world()
		update_minimap()
	elseif event == "WORLD_MAP_UPDATE" then
		update_world()
	end
end)
local since = 0
frame:SetScript("OnUpdate", function()
	since = since + (arg1 or 0)
	if since > 0.2 then
		since = 0
		update_minimap()
	end
end)

SendChatMessage(".markers", "SAY")
