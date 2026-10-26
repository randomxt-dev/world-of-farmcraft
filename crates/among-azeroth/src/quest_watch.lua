-- Among Azeroth's tasks on the stock quest tracker from the moment they are handed out. 1.12 adds
-- a quest to the tracker only when an objective advances (AutoQuestWatch_Update on
-- QUEST_WATCH_UPDATE); this watches each quest the first time it is in the log, so a quest the
-- player untracks stays untracked. Every quest in this game is a task or the crew's count.

if AmongAzeroth_QuestWatchHooked then
  return
end
AmongAzeroth_QuestWatchHooked = 1

-- Titles already offered to the tracker; a title that leaves the log may be offered again.
local offered = {}

local function watchNew()
  local limit = MAX_WATCHABLE_QUESTS or 5
  local present = {}
  local added = false
  for i = 1, GetNumQuestLogEntries() do
    local title, _, _, isHeader = GetQuestLogTitle(i)
    if title and not isHeader then
      present[title] = true
      if not offered[title] then
        offered[title] = true
        if not IsQuestWatched(i) and GetNumQuestWatches() < limit then
          AddQuestWatch(i)
          added = true
        end
      end
    end
  end
  for title in pairs(offered) do
    if not present[title] then
      offered[title] = nil
    end
  end
  if added and QuestWatch_Update then
    QuestWatch_Update()
  end
end

local frame = CreateFrame("Frame", "AmongAzerothQuestWatch")
frame:RegisterEvent("QUEST_LOG_UPDATE")
frame:SetScript("OnEvent", watchNew)
watchNew()
