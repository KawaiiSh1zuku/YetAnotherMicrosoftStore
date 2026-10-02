use yet_another_microsoft_store_lib::{
    deployment::DeploymentScope,
    deployment_coordinator::{route_for_scope, DeploymentRoute},
};

#[test]
fn current_user_scope_never_routes_to_the_broker() {
    assert_eq!(
        route_for_scope(DeploymentScope::CurrentUser),
        DeploymentRoute::CurrentUserDirect
    );
}

#[test]
fn all_users_scope_always_routes_to_the_broker() {
    assert_eq!(
        route_for_scope(DeploymentScope::AllUsers),
        DeploymentRoute::AllUsersBroker
    );
}
