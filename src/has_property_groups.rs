// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use crate::PropertyGroup;
use crate::PropertyGroupDirect;
use crate::PropertyGroupComposed;
use crate::PropertyGroups;
use crate::error::IterError;
use crate::error::LookupError;

pub trait HasDirectPropertyGroups {
    fn property_group_direct(
        &self,
        name: &str,
    ) -> Result<Option<PropertyGroup<'_, PropertyGroupDirect>>, LookupError>;

    fn property_groups_direct(
        &self,
    ) -> Result<PropertyGroups<'_, PropertyGroupDirect>, IterError>;
}

pub trait HasComposedPropertyGroups {
    fn property_group_composed(
        &self,
        name: &str,
    ) -> Result<Option<PropertyGroup<'_, PropertyGroupComposed>>, LookupError>;

    fn property_groups_composed(
        &self,
    ) -> Result<PropertyGroups<'_, PropertyGroupComposed>, IterError>;
}
