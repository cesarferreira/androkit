plugins {
    id("com.android.application")
}

android {
    namespace = "com.example.simple"
    compileSdk = 34

    defaultConfig {
        applicationId = "com.example.simple"
        minSdk = 24
        targetSdk = 34
    }

    buildTypes {
        getByName("release") {
            isMinifyEnabled = true
        }
    }
}
