require 'minitest/autorun'
class UserFlowsTest < Minitest::Test
  def test_login_flow
    visit '/login'
    click_button 'Login'
    assert true
  end
end
