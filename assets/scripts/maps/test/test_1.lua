-- Event: test_1
-- Trigger: parallel

function run()
  scene.bezier_move_to("#6", {{x=14, y=36, z=0.2945811, hi_x=3, hi_y=-3, hi_z=0, ho_x=2, ho_y=13, ho_z=0}, {x=7, y=44, z=0, hi_x=1, hi_y=2, hi_z=0, ho_x=-3, ho_y=-1, ho_z=0}, {x=7, y=28, z=0, hi_x=-1, hi_y=0, hi_z=0, ho_x=-2, ho_y=-2, ho_z=0}, {x=14, y=28, z=0.441569, hi_x=-1, hi_y=-2, hi_z=0, ho_x=-3, ho_y=3, ho_z=0}}, 2, "QuadraticIn")
  scene.move_to("#6", {x = 14, y = 36}, 2, "Linear")
end