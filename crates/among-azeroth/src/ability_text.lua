-- Among Azeroth's names and descriptions over the borrowed spells' Spell.dbc text, the way a 1.12
-- addon would: wrap GameTooltip:SetAction and SetSpell, and GetSpellName. 1.12's API never names
-- an action's spell id, so a spell is recognised by the name the stock renderer wrote.
-- AmongAzeroth_Abilities, keyed by that name, is set just before this chunk runs.

if AmongAzeroth_AbilityTextHooked then
  return
end
AmongAzeroth_AbilityTextHooked = 1

local abilities = AmongAzeroth_Abilities

-- Rewrite a tooltip the stock renderer just filled with a borrowed spell: name (and rank), the
-- cast and cooldown line, then the description in 1.12's gold.
local function retell(tip)
  local name = tip:GetName()
  local left = getglobal(name .. "TextLeft1")
  local a = left and abilities[left:GetText() or ""]
  if not a then
    return
  end
  local right = getglobal(name .. "TextRight1")
  local rank = a.rank
  if rank == nil and right and right:IsShown() then
    rank = right:GetText()
  end
  tip:ClearLines()
  if rank and rank ~= "" then
    tip:AddDoubleLine(a.name, rank, 1, 1, 1, 0.5, 0.5, 0.5)
  else
    tip:AddLine(a.name, 1, 1, 1)
  end
  tip:AddDoubleLine(a.cast, a.cooldown or "", 1, 1, 1, 1, 1, 1)
  tip:AddLine(a.description, 1, 0.82, 0, 1)
  tip:Show()
end

local function wrap(method)
  local stock = GameTooltip[method]
  GameTooltip[method] = function(self, a1, a2)
    local r1, r2 = stock(self, a1, a2)
    retell(self)
    return r1, r2
  end
end
wrap("SetAction")
wrap("SetSpell")

-- The spellbook's name and subtext.
local stockGetSpellName = GetSpellName
function GetSpellName(id, bookType)
  local name, rank = stockGetSpellName(id, bookType)
  local a = name and abilities[name]
  if not a then
    return name, rank
  end
  if a.rank == nil then
    return a.name, rank
  end
  return a.name, a.rank
end
