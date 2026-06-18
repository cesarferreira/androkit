plugins {
    id("com.android.application")
}

android {
    namespace = "com.example.flavors"
    compileSdk = 34

    defaultConfig {
        applicationId = "com.example.flavors"
        minSdk = 24
        targetSdk = 34
    }

    flavorDimensions += "env"
    productFlavors {
        create("dev") {
            dimension = "env"
        }
        create("prod") {
            dimension = "env"
        }
    }

    buildTypes {
        getByName("debug") {
        }
        getByName("release") {
            isMinifyEnabled = true
        }
    }
}
