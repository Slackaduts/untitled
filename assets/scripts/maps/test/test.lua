-- Event: TEST
-- Trigger: parallel

function run()
  scene.move_to("#6", {x = 9, y = 44}, 2, "QuadraticIn")
  scene.move_to("#6", {x = 14, y = 44}, 2, "QuadraticIn")
  scene.move_to("#6", {x = 18, y = 41}, 2, "QuadraticIn")
  scene.move_to("#6", {x = 16, y = 40}, 2, "QuadraticIn")
  scene.move_to("#6", {x = 5, y = 32}, 2, "QuadraticIn")
  scene.move_to("#6", {x = 9, y = 44}, 2, "QuadraticIn")
end