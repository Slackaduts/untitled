-- Event: TestConversation
-- Trigger: interact("#6")

function run()
  scene.run_yarn_node_at("TestConvo", {["Guard 1"] = "#6", ["Guard 2"] = "#7"}, true)
end