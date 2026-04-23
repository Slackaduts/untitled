-- Event: event_2
-- Trigger: parallel

function run()
  scene.move_to("#6", {x = 11, y = 40}, 2, "Linear")
  scene.move_to("#6", {x = 14, y = 37}, 2, "Linear")
  scene.move_to("#6", {x = 20, y = 39}, 2, "Linear")
  scene.move_to("#6", {x = 16, y = 43}, 2, "Linear")
  scene.move_to("#6", {x = 11, y = 40}, 2, "Linear")
end