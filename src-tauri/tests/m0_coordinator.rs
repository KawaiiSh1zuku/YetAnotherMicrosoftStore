use yet_another_microsoft_store_lib::{
    deployment::DeploymentScope,
    deployment_coordinator::{route_for_scope, DeploymentRoute},
    error::{classify_deployment_hresult, ErrorCode},
};

#[test]
fn native_package_in_use_hresult_is_classified_without_predictive_process_checks() {
    assert_eq!(
        classify_deployment_hresult(0x80073D02_u32 as i32),
        ErrorCode::PackageInUse
    );
    assert_eq!(
        classify_deployment_hresult(0x80070005_u32 as i32),
        ErrorCode::DeploymentDenied
    );
    assert_eq!(
        classify_deployment_hresult(0x80004005_u32 as i32),
        ErrorCode::DeploymentFailed
    );
}

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
