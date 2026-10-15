-- Among Azeroth's Lights and Comms sabotages on screen. Lights darkens a living crewmate's view
-- until fixed, as Among Us shrinks crew vision; impostors and ghosts see as before. Comms hides
-- everyone's task list (the quest tracker and quest log) until fixed. The server's notices arrive
-- as CHAT_MSG_ADDON lines; AmongAzeroth_SabotageWords, set just before this chunk runs, holds
-- the prefix and the words among-rules' wire.rs encodes them with.

if AmongAzeroth_SabotageViewHooked then
  return
end
AmongAzeroth_SabotageViewHooked = 1

local W = AmongAzeroth_SabotageWords

-- How much of the world the dark leaves: a black wash over the whole screen, the stock
-- out-of-control vignette tinted black over it so the edges go fully dark.
local SHADE = 0.75
local VIGNETTE = "Interface\\FullScreenTextures\\OutOfControl"
local COMMS_DOWN = "Comms are down: your tasks are hidden until they are fixed."

local role -- W.crew or W.impostor, from this game's role notice
local dead = false
local lights = false
local comms = false

-- Parentless, so hiding UIParent (Alt-Z) does not lift the dark; BACKGROUND strata at level 0 so
-- every stock frame, action bars, chat and the tracker, draws over it.
local dark = CreateFrame("Frame", "AmongAzerothLightsOut")
dark:SetFrameStrata("BACKGROUND")
dark:SetFrameLevel(0)
if WorldFrame then
  dark:SetAllPoints(WorldFrame)
elseif UIParent then
  dark:SetAllPoints(UIParent)
end
local shade = dark:CreateTexture("AmongAzerothLightsOutShade", "BACKGROUND")
shade:SetAllPoints(dark)
shade:SetTexture(0, 0, 0)
shade:SetAlpha(SHADE)
local edge = dark:CreateTexture("AmongAzerothLightsOutEdge", "BORDER")
edge:SetAllPoints(dark)
edge:SetTexture(VIGNETTE)
edge:SetVertexColor(0, 0, 0)
dark:Hide()

-- A crewmate voted out becomes a ghost with no notice of their own (the server's "ejected"
-- names a guid 1.12's Lua cannot compare), so the player's ghost flag counts as dead too.
local function ghost()
  return UnitIsDeadOrGhost and UnitIsDeadOrGhost("player")
end

local function refreshLights()
  if lights and role == W.crew and not dead and not ghost() then
    dark:Show()
  else
    dark:Hide()
  end
end

dark:SetScript("OnUpdate", function()
  if ghost() then
    dead = true
    dark:Hide()
  end
end)

local function hideTasks()
  if QuestWatchFrame and QuestWatchFrame:IsShown() then
    QuestWatchFrame:Hide()
  end
  if QuestLogFrame and QuestLogFrame:IsShown() then
    if HideUIPanel then
      HideUIPanel(QuestLogFrame)
    else
      QuestLogFrame:Hide()
    end
  end
end

-- Shown while Comms is down: each frame it closes whatever stock code reopened.
local blind = CreateFrame("Frame", "AmongAzerothCommsDown")
blind:SetScript("OnUpdate", hideTasks)
blind:Hide()

-- The stock tracker shows itself whenever it has watches; while Comms is down it stays hidden.
local stockQuestWatchUpdate = QuestWatch_Update
if stockQuestWatchUpdate then
  function QuestWatch_Update()
    stockQuestWatchUpdate()
    if comms and QuestWatchFrame then
      QuestWatchFrame:Hide()
    end
  end
end

-- The L key and the micro button open the log through ToggleQuestLog; closing stays allowed.
local stockToggleQuestLog = ToggleQuestLog
if stockToggleQuestLog then
  function ToggleQuestLog()
    if comms and not (QuestLogFrame and QuestLogFrame:IsVisible()) then
      if UIErrorsFrame then
        UIErrorsFrame:AddMessage(COMMS_DOWN, 1, 0.1, 0.1, 1)
      end
      return
    end
    stockToggleQuestLog()
  end
end

local function setComms(down)
  if comms == down then
    return
  end
  comms = down
  if down then
    blind:Show()
    hideTasks()
  else
    blind:Hide()
    -- The stock update shows the tracker again if it has watches.
    if QuestWatch_Update then
      QuestWatch_Update()
    end
  end
end

local function setSabotage(kind, active)
  if kind == W.lights then
    lights = active
    refreshLights()
  elseif kind == W.comms then
    setComms(active)
  end
end

-- A game's start, its end and the lobby clear everything: no sabotage outlives its game.
local function reset()
  role = nil
  dead = false
  lights = false
  refreshLights()
  setComms(false)
end

local function onNotice(message)
  local _, _, verb, what = string.find(message, "^(%S+)%s*(%S*)")
  if verb == W.role then
    role = what
    refreshLights()
  elseif verb == W.dead then
    dead = true
    refreshLights()
  elseif verb == W.sabotage then
    setSabotage(what, true)
  elseif verb == W.fixed then
    setSabotage(what, false)
  elseif verb == W.started or verb == W.over or verb == W.lobby then
    reset()
  end
end

local frame = CreateFrame("Frame", "AmongAzerothSabotageView")
frame:RegisterEvent("CHAT_MSG_ADDON")
frame:SetScript("OnEvent", function()
  if arg1 == W.prefix and arg2 then
    onNotice(arg2)
  end
end)
