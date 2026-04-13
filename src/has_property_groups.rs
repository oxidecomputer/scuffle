// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use crate::PropertyGroup;
use crate::PropertyGroups;
use crate::error::IterError;
use crate::error::LookupError;

pub trait HasPropertyGroups {
    type St;

    fn property_group(
        &self,
        name: &str,
    ) -> Result<Option<PropertyGroup<'_, Self::St>>, LookupError>;

    fn property_groups(
        &self,
    ) -> Result<PropertyGroups<'_, Self::St>, IterError>;
}
