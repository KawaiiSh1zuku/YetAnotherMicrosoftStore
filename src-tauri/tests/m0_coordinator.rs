use yet_another_microsoft_store_lib::{
    deployment::DeploymentScope,
    deployment_coordinator::{route_for_scope, DeploymentRoute},
};

#[test]
fn current_user_scope_routes_directly_in_the_elevated_process() {
    assert_eq!(
        route_for_scope(DeploymentScope::CurrentUser),
        DeploymentRoute::CurrentUserDirect
    );
}

#[test]
fn all_users_scope_routes_directly_in_the_elevated_process() {
    assert_eq!(
        route_for_scope(DeploymentScope::AllUsers),
        DeploymentRoute::AllUsersDirect
    );
}
